use super::*;

#[async_trait::async_trait]
impl ChatServiceService for ChatServer {
    async fn send_message(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::send_message(self, client_id, connection_manager, _user, request).await
    }

    async fn join_room(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: JoinRoomRequest,
    ) -> Result<JoinRoomResponse, Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::join_room(self, client_id, connection_manager, _user, request).await
    }

    async fn leave_room(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: LeaveRoomRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::leave_room(self, client_id, connection_manager, _user, request).await
    }

    async fn list_rooms(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: ListRoomsRequest,
    ) -> Result<ListRoomsResponse, Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::list_rooms(self, _client_id, _connection_manager, _user, _request).await
    }

    async fn kick_user(
        &self,
        _client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: KickUserRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::kick_user(self, _client_id, connection_manager, _user, request).await
    }

    async fn broadcast_announcement(
        &self,
        _client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: BroadcastAnnouncementRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::broadcast_announcement(self, _client_id, connection_manager, _user, request)
            .await
    }

    async fn get_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: GetProfileRequest,
    ) -> Result<GetProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::get_profile(self, _client_id, _connection_manager, _user, request).await
    }

    async fn update_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        user: &AuthenticatedUser,
        request: UpdateProfileRequest,
    ) -> Result<UpdateProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::update_profile(self, _client_id, _connection_manager, user, request).await
    }

    async fn start_typing(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: StartTypingRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::start_typing(self, client_id, connection_manager, _user, _request).await
    }

    async fn stop_typing(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: StopTypingRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        ChatServer::stop_typing(self, client_id, connection_manager, _user, _request).await
    }

    // Server-side notification hooks required by the generated trait. The chat
    // server broadcasts notifications directly through the connection manager.
    async fn notify_message_received(
        &self,
        _connection_id: ConnectionId,
        _params: MessageReceivedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_joined(
        &self,
        _connection_id: ConnectionId,
        _params: UserJoinedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_left(
        &self,
        _connection_id: ConnectionId,
        _params: UserLeftNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_system_announcement(
        &self,
        _connection_id: ConnectionId,
        _params: SystemAnnouncementNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_kicked(
        &self,
        _connection_id: ConnectionId,
        _params: UserKickedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_room_created(
        &self,
        _connection_id: ConnectionId,
        _params: RoomCreatedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_room_deleted(
        &self,
        _connection_id: ConnectionId,
        _params: RoomDeletedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_started_typing(
        &self,
        _connection_id: ConnectionId,
        _params: UserStartedTypingNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_stopped_typing(
        &self,
        _connection_id: ConnectionId,
        _params: UserStoppedTypingNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    // Lifecycle hooks
    async fn on_client_connected(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Client {} connected", client_id);

        // Send welcome message
        let notification = SystemAnnouncementNotification {
            message: "Welcome to the chat server! Please authenticate to continue.".to_string(),
            level: AnnouncementLevel::Info,
            timestamp: Utc::now().to_rfc3339(),
        };

        let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
            method: "system_announcement".to_string(),
            params: serde_json::to_value(&notification).unwrap(),
            metadata: None,
        };
        let msg = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(
            notification_msg,
        );
        if let Err(e) = connection_manager.send_to_connection(client_id, msg).await {
            warn!(
                "Failed to send welcome message to client {}: {:?}",
                client_id, e
            );
        }

        Ok(())
    }

    async fn on_client_disconnected(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Client {} disconnected", client_id);

        // Remove user session and notify room members
        if let Some((_, session)) = self.user_sessions.remove(&client_id) {
            let username = session.username.clone();
            self.clear_message_rate_limit(&username).await;

            if let Some(room_id) = session.current_room {
                // Clear typing state if user was typing
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
                    self.broadcast_typing_notification(
                        connection_manager,
                        &room_id,
                        &username,
                        false,
                    )
                    .await;
                }

                // Remove from room
                if let Some(mut room) = self.rooms.get_mut(&room_id) {
                    room.users.remove(&session.username);
                    let user_count = room.users.len() as u32;
                    let room_users: Vec<String> = room.users.iter().cloned().collect();
                    drop(room);

                    // Notify remaining users
                    let notification = UserLeftNotification {
                        username: session.username,
                        room_id,
                        user_count,
                    };

                    for target_username in room_users {
                        for entry in self.user_sessions.iter() {
                            if entry.username == target_username {
                                let notification_msg =
                                    ras_jsonrpc_bidirectional_types::ServerNotification {
                                        method: "user_left".to_string(),
                                        params: serde_json::to_value(&notification).unwrap(),
                                        metadata: None,
                                    };
                                let msg = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(notification_msg);
                                if let Err(e) = connection_manager
                                    .send_to_connection(*entry.key(), msg)
                                    .await
                                {
                                    warn!(connection_id = %entry.key(),
                                          "Failed to send user_left notification on disconnect: {:?}", e);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn on_client_authenticated(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        user: &AuthenticatedUser,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Client {} authenticated as user {}",
            client_id, user.user_id
        );

        // Create user session
        let session = UserSession {
            username: user.user_id.clone(),
            current_room: None,
        };

        self.user_sessions.insert(client_id, session);

        // Send personalized welcome
        let notification = SystemAnnouncementNotification {
            message: format!(
                "Welcome {}, you have been successfully authenticated!",
                user.user_id
            ),
            level: AnnouncementLevel::Info,
            timestamp: Utc::now().to_rfc3339(),
        };

        let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
            method: "system_announcement".to_string(),
            params: serde_json::to_value(&notification).unwrap(),
            metadata: None,
        };
        let msg = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(
            notification_msg,
        );
        if let Err(e) = connection_manager.send_to_connection(client_id, msg).await {
            warn!(
                "Failed to send welcome message to client {}: {:?}",
                client_id, e
            );
        }

        Ok(())
    }
}

// Permission provider for the chat application
