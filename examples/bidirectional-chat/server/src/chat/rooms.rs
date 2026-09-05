use super::*;

impl ChatServer {
    #[instrument(skip(self, connection_manager, _user), fields(client_id = %client_id, user = %_user.user_id, room_name = %request.room_name))]
    pub(super) async fn join_room(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: JoinRoomRequest,
    ) -> Result<JoinRoomResponse, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Processing join_room request");

        // Validate room name length
        if request.room_name.len() > self.config.max_room_name_length {
            return Err(format!(
                "Room name too long. Maximum length is {} characters",
                self.config.max_room_name_length
            )
            .into());
        }

        // Get or create room
        let room_id = if self.rooms.contains_key(&request.room_name) {
            request.room_name.clone()
        } else {
            // Create new room
            let room_id = if request.room_name.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                request.room_name.clone()
            };

            let new_room = ChatRoom {
                id: room_id.clone(),
                name: request.room_name.clone(),
                users: HashSet::new(),
                created_at: Utc::now(),
            };

            self.rooms.insert(room_id.clone(), new_room.clone());

            // Persist new room
            let mut state = self.persistence.load_state().await.unwrap_or_default();
            state.rooms.insert(
                room_id.clone(),
                PersistedRoom {
                    id: new_room.id.clone(),
                    name: new_room.name.clone(),
                    created_at: new_room.created_at,
                    users: new_room.users.clone(),
                },
            );
            if let Err(e) = self.persistence.save_state(&state).await {
                error!(room_id = %room_id, "Failed to persist new room: {}", e);
            } else {
                info!(room_id = %room_id, room_name = %new_room.name, "New room created and persisted");
            }

            // Notify all users about new room
            let room_info = self.get_room_info(&room_id).unwrap();
            let notification = RoomCreatedNotification { room_info };

            // Broadcast to all connected users
            for entry in self.user_sessions.iter() {
                let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
                    method: "room_created".to_string(),
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
                          "Failed to send room_created notification: {:?}", e);
                }
            }

            room_id
        };

        // Get user session
        let mut session = self.user_sessions.get_mut(&client_id).ok_or_else(|| {
            error!("User session not found for client {}", client_id);
            "User session not found"
        })?;

        let username = session.username.clone();

        // Leave current room if in one
        if let Some(current_room_id) = &session.current_room
            && let Some(mut room) = self.rooms.get_mut(current_room_id)
        {
            room.users.remove(&username);
            let user_count = room.users.len() as u32;
            drop(room);

            // Notify users in old room
            let notification = UserLeftNotification {
                username: username.clone(),
                room_id: current_room_id.clone(),
                user_count,
            };

            for entry in self.user_sessions.iter() {
                if entry.current_room.as_ref() == Some(current_room_id) {
                    let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
                        method: "user_left".to_string(),
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
                        warn!(connection_id = %entry.key(),
                              "Failed to send user_left notification: {:?}", e);
                    }
                }
            }
        }

        // Update session
        session.current_room = Some(room_id.clone());
        drop(session);

        // Add user to new room
        let mut room = self.rooms.get_mut(&room_id).ok_or("Room not found")?;

        // Check user limit
        if self.config.max_users_per_room > 0 && room.users.len() >= self.config.max_users_per_room
        {
            return Err(format!(
                "Room is full. Maximum {} users allowed per room",
                self.config.max_users_per_room
            )
            .into());
        }

        // Get existing users before adding the new user
        let existing_users: Vec<String> = room.users.iter().cloned().collect();

        room.users.insert(username.clone());
        let user_count = room.users.len() as u32;
        let room_users: Vec<String> = room.users.iter().cloned().collect();
        drop(room);

        // Notify users in new room
        let notification = UserJoinedNotification {
            username: username.clone(),
            room_id: room_id.clone(),
            user_count,
        };

        for target_username in room_users {
            for entry in self.user_sessions.iter() {
                if entry.username == target_username {
                    let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
                        method: "user_joined".to_string(),
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

        info!(
            user = %username,
            room_id = %room_id,
            existing_users = ?existing_users,
            user_count = %user_count,
            "User joined room successfully"
        );

        Ok(JoinRoomResponse {
            room_id,
            user_count,
            existing_users,
        })
    }
    pub(super) async fn leave_room(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: LeaveRoomRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut session = self
            .user_sessions
            .get_mut(&client_id)
            .ok_or("User session not found")?;

        // Check if user is in the requested room
        if session.current_room.as_ref() != Some(&request.room_id) {
            return Err("User not in the specified room".into());
        }

        let username = session.username.clone();
        let room_id_for_log = request.room_id.clone();
        session.current_room = None;
        drop(session);

        // Remove user from room
        if let Some(mut room) = self.rooms.get_mut(&request.room_id) {
            room.users.remove(&username);
            let user_count = room.users.len() as u32;
            let room_users: Vec<String> = room.users.iter().cloned().collect();
            drop(room);

            // Notify remaining users
            let notification = UserLeftNotification {
                username: username.clone(),
                room_id: request.room_id,
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
                                  "Failed to send user_left notification: {:?}", e);
                        }
                    }
                }
            }
        }

        info!(user = %username, room_id = %room_id_for_log, "User left room successfully");
        Ok(())
    }
    #[instrument(skip(self, _connection_manager, _user), fields(client_id = %_client_id, user = %_user.user_id))]
    pub(super) async fn list_rooms(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: ListRoomsRequest,
    ) -> Result<ListRoomsResponse, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Processing list_rooms request");
        let rooms: Vec<RoomInfo> = self
            .rooms
            .iter()
            .map(|entry| RoomInfo {
                room_id: entry.id.clone(),
                room_name: entry.name.clone(),
                user_count: entry.users.len() as u32,
            })
            .collect();

        debug!(room_count = rooms.len(), "Returning room list");
        Ok(ListRoomsResponse { rooms })
    }
    pub(super) async fn kick_user(
        &self,
        _client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: KickUserRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Find the target user's session
        let mut target_connection_id = None;
        let mut target_room_id = None;

        for entry in self.user_sessions.iter() {
            if entry.username == request.target_username {
                target_connection_id = Some(*entry.key());
                target_room_id = entry.current_room.clone();
                break;
            }
        }

        let target_id = target_connection_id.ok_or("Target user not found")?;

        // Remove user from their room if they're in one
        if let Some(ref room_id) = target_room_id
            && let Some(mut room) = self.rooms.get_mut(room_id)
        {
            room.users.remove(&request.target_username);
        }

        // Send kick notification to the target user
        let kick_notification = UserKickedNotification {
            username: request.target_username.clone(),
            reason: request.reason.clone(),
            room_id: target_room_id.as_ref().cloned().unwrap_or_default(),
        };

        let notification_msg = ras_jsonrpc_bidirectional_types::ServerNotification {
            method: "user_kicked".to_string(),
            params: serde_json::to_value(&kick_notification).unwrap(),
            metadata: None,
        };
        let msg = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(
            notification_msg,
        );
        if let Err(e) = connection_manager.send_to_connection(target_id, msg).await {
            warn!("Failed to send kick notification to user: {:?}", e);
        }

        // Remove the user's session
        self.user_sessions.remove(&target_id);
        self.clear_message_rate_limit(&request.target_username)
            .await;
        debug!("Removed user session for {}", request.target_username);

        // Disconnect the user
        if let Err(e) = connection_manager.remove_connection(target_id).await {
            warn!("Failed to disconnect user: {:?}", e);
        }

        Ok(true)
    }
}
