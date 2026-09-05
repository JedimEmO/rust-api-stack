use super::*;

impl ChatServer {
    #[instrument(skip(self, connection_manager, _user), fields(client_id = %client_id, user = %_user.user_id))]
    pub(super) async fn send_message(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Processing send_message request");

        // Validate message length
        if request.text.len() > self.config.max_message_length {
            return Err(format!(
                "Message too long. Maximum length is {} characters",
                self.config.max_message_length
            )
            .into());
        }

        // Get user session
        let session = self.user_sessions.get(&client_id).ok_or_else(|| {
            error!("User session not found for client {}", client_id);
            "User session not found"
        })?;

        let room_id = session.current_room.clone().ok_or_else(|| {
            warn!("User {} not in any room", session.username);
            "User not in any room"
        })?;

        // Drop the session ref to avoid holding the lock
        let username = session.username.clone();
        drop(session);

        self.check_message_rate_limit(&username).await?;

        // Clear typing state when sending a message
        let mut typing_users = self.typing_users.lock().await;
        let mut was_typing = false;
        if let Some(room_typing_users) = typing_users.get_mut(&room_id) {
            if room_typing_users.remove(&username).is_some() {
                was_typing = true;
            }
            if room_typing_users.is_empty() {
                typing_users.remove(&room_id);
            }
        }
        drop(typing_users);

        // Send stop typing notification if user was typing
        if was_typing {
            self.broadcast_typing_notification(connection_manager, &room_id, &username, false)
                .await;
        }

        // Get room to find all users
        let room = self.rooms.get(&room_id).ok_or_else(|| {
            error!("Room {} not found", room_id);
            "Room not found"
        })?;
        let room_users: Vec<String> = room.users.iter().cloned().collect();
        let user_count = room.users.len();
        drop(room);

        debug!(room_id = %room_id, user_count = user_count, "Broadcasting message to room");

        // Generate message details
        let message_id = self.next_message_id().await;
        let timestamp = Utc::now();
        let timestamp_str = timestamp.to_rfc3339();

        // Create notification
        let notification = MessageReceivedNotification {
            message_id,
            username: username.clone(),
            text: request.text.clone(),
            timestamp: timestamp_str.clone(),
            room_id: room_id.clone(),
        };

        // Persist message to disk
        let persisted_msg = PersistedMessage {
            id: message_id,
            room_id: room_id.clone(),
            username: username.clone(),
            text: request.text,
            timestamp,
        };
        if let Err(e) = self
            .persistence
            .append_message(&room_id, &persisted_msg)
            .await
        {
            error!(message_id = message_id, room_id = %room_id, "Failed to persist message: {}", e);
        } else {
            debug!(message_id = message_id, "Message persisted successfully");
        }

        // Send to all users in the room
        for target_username in room_users {
            // Find connection ID for this username
            for entry in self.user_sessions.iter() {
                if entry.username == target_username {
                    // Send notification directly using connection manager
                    let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
                        method: "message_received".to_string(),
                        params: serde_json::to_value(&notification).unwrap(),
                        metadata: None,
                    };
                    let msg =
                        ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(
                            notification_msg,
                        );
                    if let Err(e) = connection_manager
                        .send_to_connection(*entry.key(), msg)
                        .await
                    {
                        warn!(target_user = %target_username, connection_id = %entry.key(),
                              "Failed to send message notification: {:?}", e);
                    }
                }
            }
        }

        info!(message_id = message_id, room_id = %room_id, sender = %username,
              "Message sent successfully");
        Ok(SendMessageResponse {
            message_id,
            timestamp: timestamp_str,
        })
    }
    pub(super) async fn broadcast_announcement(
        &self,
        _client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: BroadcastAnnouncementRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let notification = SystemAnnouncementNotification {
            message: request.message,
            level: request.level,
            timestamp: Utc::now().to_rfc3339(),
        };

        // Send to all connected users
        for entry in self.user_sessions.iter() {
            let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
                method: "system_announcement".to_string(),
                params: serde_json::to_value(&notification).unwrap(),
                metadata: None,
            };
            let msg = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(
                notification_msg,
            );
            if let Err(e) = connection_manager
                .send_to_connection(*entry.key(), msg)
                .await
            {
                warn!(connection_id = %entry.key(),
                      "Failed to send announcement: {:?}", e);
            }
        }

        let user_count = self.user_sessions.len();
        info!(user_count = user_count, "Announcement broadcast complete");
        Ok(())
    }
}
