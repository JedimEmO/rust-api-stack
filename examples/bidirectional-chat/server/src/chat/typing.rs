use super::*;

impl ChatServer {
    pub(super) async fn start_typing(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: StartTypingRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get user session
        let session = self.user_sessions.get(&client_id).ok_or_else(|| {
            error!("User session not found for client {}", client_id);
            "User session not found"
        })?;

        let username = session.username.clone();
        let room_id = session.current_room.clone().ok_or_else(|| {
            warn!("User {} not in any room", session.username);
            "User not in any room"
        })?;
        drop(session);

        // Update typing state
        let mut typing_users = self.typing_users.lock().await;
        let room_typing_users = typing_users
            .entry(room_id.clone())
            .or_insert_with(HashMap::new);

        let is_new_typing = !room_typing_users.contains_key(&username);
        room_typing_users.insert(
            username.clone(),
            TypingState {
                started_at: Instant::now(),
            },
        );
        drop(typing_users);

        // Send notification only if this is a new typing state
        if is_new_typing {
            self.broadcast_typing_notification(connection_manager, &room_id, &username, true)
                .await;
        }

        // Clean up expired typing states
        self.cleanup_expired_typing_states(connection_manager).await;

        Ok(())
    }
    pub(super) async fn stop_typing(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: StopTypingRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get user session
        let session = self.user_sessions.get(&client_id).ok_or_else(|| {
            error!("User session not found for client {}", client_id);
            "User session not found"
        })?;

        let username = session.username.clone();
        let room_id = session.current_room.clone().ok_or_else(|| {
            warn!("User {} not in any room", session.username);
            "User not in any room"
        })?;
        drop(session);

        // Remove from typing state
        let mut typing_users = self.typing_users.lock().await;
        let mut should_notify = false;

        if let Some(room_typing_users) = typing_users.get_mut(&room_id) {
            if room_typing_users.remove(&username).is_some() {
                should_notify = true;
            }

            // Clean up empty room entries
            if room_typing_users.is_empty() {
                typing_users.remove(&room_id);
            }
        }
        drop(typing_users);

        // Send notification if user was typing
        if should_notify {
            self.broadcast_typing_notification(connection_manager, &room_id, &username, false)
                .await;
        }

        Ok(())
    }
}
