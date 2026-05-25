//! Bidirectional chat server example
//!
//! This example demonstrates a real-time chat server using bidirectional JSON-RPC over WebSockets.
//! Features include:
//! - Multiple chat rooms
//! - User authentication with JWT
//! - Role-based permissions (user, moderator, admin)
//! - Real-time message broadcasting
//! - System announcements
//! - User management (kick functionality)

use anyhow::Result;
use axum::{Router, routing::get};
use bidirectional_chat_api::auth::{
    ChatAuthServiceBuilder, HealthResponse, LoginRequest, LoginResponse, RegisterRequest,
    RegisterResponse,
};
use bidirectional_chat_api::*;
use chrono::Utc;
use dashmap::DashMap;
use ras_auth_core::AuthenticatedUser;
use ras_identity_core::{UserPermissions, VerifiedIdentity};
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAlgorithm, JwtAuthProvider, SessionConfig, SessionService};
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager, WebSocketServiceBuilder,
    service::{BuiltWebSocketService, websocket_handler},
};
use ras_jsonrpc_bidirectional_types::{ConnectionId, ConnectionManager};
use ras_rest_core::{RestError, RestResponse, RestResult};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use bidirectional_chat_server::config::{self, Config};
use bidirectional_chat_server::persistence::{
    PersistedCatAvatar, PersistedMessage, PersistedRoom, PersistedUserProfile, PersistenceManager,
};

// Chat room state
#[derive(Debug, Clone)]
struct ChatRoom {
    id: String,
    name: String,
    users: HashSet<String>, // usernames
    created_at: chrono::DateTime<Utc>,
}

// User session state
#[derive(Debug, Clone)]
struct UserSession {
    username: String,
    current_room: Option<String>, // room_id
}

// Typing state tracking
#[derive(Debug, Clone)]
struct TypingState {
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct MessageRateLimitState {
    window_start: Instant,
    messages_sent: u32,
}

fn persisted_cat_breed(breed: CatBreed) -> &'static str {
    match breed {
        CatBreed::Tabby => "tabby",
        CatBreed::Siamese => "siamese",
        CatBreed::Persian => "persian",
        CatBreed::MaineCoon => "maine_coon",
        CatBreed::BritishShorthair => "british_shorthair",
        CatBreed::Ragdoll => "ragdoll",
        CatBreed::Sphynx => "sphynx",
        CatBreed::ScottishFold => "scottish_fold",
        CatBreed::Calico => "calico",
        CatBreed::Tuxedo => "tuxedo",
    }
}

fn persisted_cat_color(color: CatColor) -> &'static str {
    match color {
        CatColor::Orange => "orange",
        CatColor::Black => "black",
        CatColor::White => "white",
        CatColor::Gray => "gray",
        CatColor::Brown => "brown",
        CatColor::Cream => "cream",
        CatColor::Blue => "blue",
        CatColor::Lilac => "lilac",
        CatColor::Cinnamon => "cinnamon",
        CatColor::Fawn => "fawn",
    }
}

fn persisted_cat_expression(expression: CatExpression) -> &'static str {
    match expression {
        CatExpression::Happy => "happy",
        CatExpression::Sleepy => "sleepy",
        CatExpression::Curious => "curious",
        CatExpression::Playful => "playful",
        CatExpression::Content => "content",
        CatExpression::Alert => "alert",
        CatExpression::Grumpy => "grumpy",
        CatExpression::Loving => "loving",
    }
}

fn cat_breed_from_persisted(value: &str) -> CatBreed {
    match value {
        "tabby" => CatBreed::Tabby,
        "siamese" => CatBreed::Siamese,
        "persian" => CatBreed::Persian,
        "maine_coon" => CatBreed::MaineCoon,
        "british_shorthair" => CatBreed::BritishShorthair,
        "ragdoll" => CatBreed::Ragdoll,
        "sphynx" => CatBreed::Sphynx,
        "scottish_fold" => CatBreed::ScottishFold,
        "calico" => CatBreed::Calico,
        "tuxedo" => CatBreed::Tuxedo,
        _ => CatBreed::Tabby,
    }
}

fn cat_color_from_persisted(value: &str) -> CatColor {
    match value {
        "orange" => CatColor::Orange,
        "black" => CatColor::Black,
        "white" => CatColor::White,
        "gray" => CatColor::Gray,
        "brown" => CatColor::Brown,
        "cream" => CatColor::Cream,
        "blue" => CatColor::Blue,
        "lilac" => CatColor::Lilac,
        "cinnamon" => CatColor::Cinnamon,
        "fawn" => CatColor::Fawn,
        _ => CatColor::Orange,
    }
}

fn cat_expression_from_persisted(value: &str) -> CatExpression {
    match value {
        "happy" => CatExpression::Happy,
        "sleepy" => CatExpression::Sleepy,
        "curious" => CatExpression::Curious,
        "playful" => CatExpression::Playful,
        "content" => CatExpression::Content,
        "alert" => CatExpression::Alert,
        "grumpy" => CatExpression::Grumpy,
        "loving" => CatExpression::Loving,
        _ => CatExpression::Happy,
    }
}

fn user_profile_from_persisted(persisted: &PersistedUserProfile) -> UserProfile {
    UserProfile {
        username: persisted.username.clone(),
        display_name: persisted.display_name.clone(),
        avatar: CatAvatar {
            breed: cat_breed_from_persisted(&persisted.avatar.breed),
            color: cat_color_from_persisted(&persisted.avatar.color),
            expression: cat_expression_from_persisted(&persisted.avatar.expression),
        },
        created_at: persisted.created_at.to_rfc3339(),
        last_seen: persisted.last_seen.to_rfc3339(),
    }
}

// Chat server state
#[derive(Clone)]
struct ChatServer {
    rooms: Arc<DashMap<String, ChatRoom>>,
    user_sessions: Arc<DashMap<ConnectionId, UserSession>>,
    message_counter: Arc<RwLock<u64>>,
    persistence: Arc<PersistenceManager>,
    config: config::ChatConfig,
    rate_limit: config::RateLimitConfig,
    typing_users: Arc<Mutex<HashMap<String, HashMap<String, TypingState>>>>, // room_id -> username -> typing state
    message_rate_limits: Arc<Mutex<HashMap<String, MessageRateLimitState>>>,
}

impl ChatServer {
    #[instrument(skip_all, fields(data_dir = ?config.data_dir, rate_limit_enabled = rate_limit.enabled))]
    async fn new_with_rate_limit(
        config: config::ChatConfig,
        rate_limit: config::RateLimitConfig,
    ) -> Result<Self> {
        info!("Initializing chat server with data directory");
        let persistence = Arc::new(PersistenceManager::new(&config.data_dir));
        persistence.init().await.map_err(|e| {
            error!("Failed to initialize persistence: {}", e);
            e
        })?;

        // Load persisted state
        debug!("Loading persisted state");
        let mut state = persistence.load_state().await.map_err(|e| {
            error!("Failed to load persisted state: {}", e);
            e
        })?;

        let server = Self {
            rooms: Arc::new(DashMap::new()),
            user_sessions: Arc::new(DashMap::new()),
            message_counter: Arc::new(RwLock::new(state.next_message_id)),
            persistence,
            config: config.clone(),
            rate_limit,
            typing_users: Arc::new(Mutex::new(HashMap::new())),
            message_rate_limits: Arc::new(Mutex::new(HashMap::new())),
        };

        // Restore rooms
        if state.rooms.is_empty() {
            info!("No rooms found in persistence, creating default rooms");
            // Create default rooms from configuration
            for room_config in &config.default_rooms {
                let room = ChatRoom {
                    id: room_config.id.clone(),
                    name: room_config.name.clone(),
                    users: HashSet::new(),
                    created_at: Utc::now(),
                };
                server.rooms.insert(room_config.id.clone(), room.clone());

                // Persist the room
                state.rooms.insert(
                    room_config.id.clone(),
                    PersistedRoom {
                        id: room.id,
                        name: room.name,
                        created_at: room.created_at,
                        users: room.users.clone(),
                    },
                );
                info!(
                    "Created default room: {} ({})",
                    room_config.name, room_config.id
                );
            }

            if !state.rooms.is_empty() {
                server.persistence.save_state(&state).await.map_err(|e| {
                    error!("Failed to save initial state: {}", e);
                    e
                })?;
            }
        } else {
            info!("Restoring {} rooms from persistence", state.rooms.len());
            // Restore rooms from persistence (clear user lists as they're not currently connected)
            for (id, persisted_room) in state.rooms {
                debug!(room_id = %id, room_name = %persisted_room.name, "Restoring room");
                let room = ChatRoom {
                    id: persisted_room.id,
                    name: persisted_room.name,
                    users: HashSet::new(), // Clear users on restart
                    created_at: persisted_room.created_at,
                };
                server.rooms.insert(id, room);
            }
        }

        Ok(server)
    }

    async fn next_message_id(&self) -> u64 {
        let mut counter = self.message_counter.write().await;
        let id = *counter;
        *counter += 1;
        id
    }

    fn get_room_info(&self, room_id: &str) -> Option<RoomInfo> {
        self.rooms.get(room_id).map(|room| RoomInfo {
            room_id: room.id.clone(),
            room_name: room.name.clone(),
            user_count: room.users.len() as u32,
        })
    }

    async fn check_message_rate_limit(
        &self,
        username: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.rate_limit.enabled {
            return Ok(());
        }

        if self.rate_limit.messages_per_minute == 0 {
            return Err("Message rate limit is configured with zero messages per minute".into());
        }

        let now = Instant::now();
        let window = Duration::from_secs(60);
        let mut limits = self.message_rate_limits.lock().await;
        let state = limits
            .entry(username.to_string())
            .or_insert_with(|| MessageRateLimitState {
                window_start: now,
                messages_sent: 0,
            });

        if now.duration_since(state.window_start) >= window {
            state.window_start = now;
            state.messages_sent = 0;
        }

        if state.messages_sent >= self.rate_limit.messages_per_minute {
            return Err(format!(
                "Rate limit exceeded. Maximum {} messages per minute",
                self.rate_limit.messages_per_minute
            )
            .into());
        }

        state.messages_sent += 1;
        Ok(())
    }

    async fn clear_message_rate_limit(&self, username: &str) {
        if self.rate_limit.enabled {
            self.message_rate_limits.lock().await.remove(username);
        }
    }

    // Clean up expired typing states (older than 5 seconds)
    async fn cleanup_expired_typing_states(&self, connection_manager: &dyn ConnectionManager) {
        let mut typing_users = self.typing_users.lock().await;
        let now = Instant::now();
        let timeout = Duration::from_secs(5);

        let mut expired_users = Vec::new();

        for (room_id, room_typing_users) in typing_users.iter_mut() {
            room_typing_users.retain(|username, state| {
                if now.duration_since(state.started_at) > timeout {
                    expired_users.push((room_id.clone(), username.clone()));
                    false
                } else {
                    true
                }
            });
        }

        drop(typing_users);

        // Send stop typing notifications for expired users
        for (room_id, username) in expired_users {
            self.broadcast_typing_notification(connection_manager, &room_id, &username, false)
                .await;
        }
    }

    // Broadcast typing notification to all users in a room
    async fn broadcast_typing_notification(
        &self,
        connection_manager: &dyn ConnectionManager,
        room_id: &str,
        username: &str,
        is_typing: bool,
    ) {
        if let Some(room) = self.rooms.get(room_id) {
            let room_users: Vec<String> = room.users.iter().cloned().collect();
            drop(room);

            let notification = if is_typing {
                let notification = UserStartedTypingNotification {
                    username: username.to_string(),
                    room_id: room_id.to_string(),
                };
                ras_jsonrpc_bidirectional_types::ServerNotification {
                    method: "user_started_typing".to_string(),
                    params: serde_json::to_value(&notification).unwrap(),
                    metadata: None,
                }
            } else {
                let notification = UserStoppedTypingNotification {
                    username: username.to_string(),
                    room_id: room_id.to_string(),
                };
                ras_jsonrpc_bidirectional_types::ServerNotification {
                    method: "user_stopped_typing".to_string(),
                    params: serde_json::to_value(&notification).unwrap(),
                    metadata: None,
                }
            };

            let msg = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(
                notification,
            );

            // Send to all users in the room except the typing user
            for target_username in room_users {
                if target_username != username {
                    for entry in self.user_sessions.iter() {
                        if entry.username == target_username
                            && let Err(e) = connection_manager
                                .send_to_connection(*entry.key(), msg.clone())
                                .await
                        {
                            warn!(target_user = %target_username, connection_id = %entry.key(),
                                  "Failed to send typing notification: {:?}", e);
                        }
                    }
                }
            }
        }
    }
}

// Implement the chat service
#[async_trait::async_trait]
impl ChatServiceService for ChatServer {
    #[instrument(skip(self, connection_manager, _user), fields(client_id = %client_id, user = %_user.user_id))]
    async fn send_message(
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

    #[instrument(skip(self, connection_manager, _user), fields(client_id = %client_id, user = %_user.user_id, room_name = %request.room_name))]
    async fn join_room(
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

    async fn leave_room(
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
    async fn list_rooms(
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

    async fn kick_user(
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

    async fn broadcast_announcement(
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

    async fn get_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: GetProfileRequest,
    ) -> Result<GetProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Load current state
        let state = self.persistence.load_state().await?;

        // Get profile from persistence or create default
        let profile = if let Some(persisted) = state.user_profiles.get(&request.username) {
            user_profile_from_persisted(persisted)
        } else {
            // Create default profile
            UserProfile {
                username: request.username.clone(),
                display_name: None,
                avatar: CatAvatar {
                    breed: CatBreed::Tabby,
                    color: CatColor::Orange,
                    expression: CatExpression::Happy,
                },
                created_at: Utc::now().to_rfc3339(),
                last_seen: Utc::now().to_rfc3339(),
            }
        };

        Ok(GetProfileResponse { profile })
    }

    async fn update_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        user: &AuthenticatedUser,
        request: UpdateProfileRequest,
    ) -> Result<UpdateProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Load current state
        let mut state = self.persistence.load_state().await?;

        // Get existing profile or create new one
        let mut persisted_profile = state
            .user_profiles
            .get(&user.user_id)
            .cloned()
            .unwrap_or_else(|| PersistedUserProfile {
                username: user.user_id.clone(),
                display_name: None,
                avatar: PersistedCatAvatar {
                    breed: "tabby".to_string(),
                    color: "orange".to_string(),
                    expression: "happy".to_string(),
                },
                created_at: Utc::now(),
                last_seen: Utc::now(),
            });

        // Update fields if provided
        if let Some(display_name) = request.display_name {
            persisted_profile.display_name = Some(display_name);
        }

        if let Some(avatar) = request.avatar {
            persisted_profile.avatar = PersistedCatAvatar {
                breed: persisted_cat_breed(avatar.breed).to_string(),
                color: persisted_cat_color(avatar.color).to_string(),
                expression: persisted_cat_expression(avatar.expression).to_string(),
            };
        }

        // Update last seen
        persisted_profile.last_seen = Utc::now();

        // Save to persistence
        state
            .user_profiles
            .insert(user.user_id.clone(), persisted_profile.clone());
        self.persistence.save_state(&state).await?;

        // Convert to response
        let profile = user_profile_from_persisted(&persisted_profile);

        Ok(UpdateProfileResponse { profile })
    }

    async fn start_typing(
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

    async fn stop_typing(
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
#[derive(Clone)]
struct ChatPermissions {
    admin_users: Vec<config::AdminUser>,
}

// REST API handlers
#[derive(Clone)]
struct AuthHandlers {
    session_service: Arc<SessionService>,
    identity_provider: Arc<LocalUserProvider>,
}

impl ChatPermissions {
    fn new(admin_users: Vec<config::AdminUser>) -> Self {
        Self { admin_users }
    }
}

#[async_trait::async_trait]
impl UserPermissions for ChatPermissions {
    async fn get_permissions(
        &self,
        identity: &VerifiedIdentity,
    ) -> ras_identity_core::IdentityResult<Vec<String>> {
        // Check if user is in admin configuration
        for admin_user in &self.admin_users {
            if admin_user.username == identity.subject {
                return Ok(admin_user.permissions.clone());
            }
        }

        // Default permissions for regular users
        Ok(vec!["user".to_string()])
    }
}

impl AuthHandlers {
    async fn handle_login(&self, request: LoginRequest) -> RestResult<LoginResponse> {
        debug!("Processing login request");

        // Create auth payload
        let provider_id = request.provider.as_deref().unwrap_or("local");
        let auth_payload = json!({
            "username": request.username,
            "password": request.password,
            "provider": provider_id,
        });

        // Begin session
        let token = self
            .session_service
            .begin_session(provider_id, auth_payload)
            .await
            .map_err(|e| {
                warn!(provider = %provider_id, "Login failed: {}", e);
                RestError::unauthorized("Invalid credentials")
            })?;

        // Parse token to get user info (for response)
        let claims = self
            .session_service
            .verify_session(&token)
            .await
            .map_err(|e| {
                warn!("Token verification failed: {}", e);
                RestError::internal_server_error("Token verification failed")
            })?;

        info!(user_id = %claims.sub, "User logged in successfully");
        Ok(RestResponse::ok(LoginResponse {
            token,
            expires_at: claims.exp,
            user_id: claims.sub,
        }))
    }

    async fn handle_register(&self, request: RegisterRequest) -> RestResult<RegisterResponse> {
        debug!("Processing registration request");

        // Add user
        self.identity_provider
            .add_user(
                request.username.clone(),
                request.password,
                request.email.clone(),
                request.display_name.clone(),
            )
            .await
            .map_err(|e| {
                warn!(username = %request.username, "Registration failed: {}", e);
                RestError::conflict("Username already exists")
            })?;

        info!(username = %request.username, email = ?request.email, "User registered successfully");

        Ok(RestResponse::created(RegisterResponse {
            message: "User registered successfully".to_string(),
            username: request.username,
            display_name: request.display_name,
        }))
    }

    async fn handle_health(&self) -> RestResult<HealthResponse> {
        Ok(RestResponse::ok(HealthResponse {
            status: "OK".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }))
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables first (before config loading)
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("No .env file found or error loading: {}", e);
    }

    // Load configuration
    let config = Config::load().map_err(|e| {
        eprintln!("Failed to load configuration: {}", e);
        e
    })?;

    // Initialize tracing based on configuration
    use tracing_subscriber::{EnvFilter, fmt};

    let subscriber = fmt::Subscriber::builder()
        .with_env_filter(EnvFilter::new(config.log_filter()))
        .with_target(config.logging.target)
        .with_thread_ids(config.logging.thread_ids)
        .with_line_number(config.logging.line_numbers)
        .with_level(true)
        .with_ansi(true);

    // Apply format settings
    match config.logging.format.as_str() {
        "json" => {
            subscriber.with_ansi(false).init();
        }
        "compact" => {
            subscriber.compact().init();
        }
        _ => {
            // "pretty" or default
            subscriber.pretty().init();
        }
    }

    info!("Starting bidirectional chat server");
    info!("Configuration loaded from environment and config file");

    // Create identity provider - use Arc to share between session service and registration
    info!("Setting up identity provider");
    let identity_provider = Arc::new(LocalUserProvider::new());

    // Add admin users from configuration
    if config.admin.auto_create {
        for admin_user in &config.admin.users {
            match identity_provider
                .add_user(
                    admin_user.username.clone(),
                    admin_user.password.clone(),
                    admin_user.email.clone(),
                    admin_user.display_name.clone(),
                )
                .await
            {
                Ok(_) => info!("Created admin user: {}", admin_user.username),
                Err(e) => {
                    // User might already exist, which is fine
                    debug!(
                        "Admin user {} might already exist: {}",
                        admin_user.username, e
                    );
                }
            }
        }
    }

    // Add some default test users if in development mode
    if cfg!(debug_assertions) {
        let test_users = vec![
            (
                "alice",
                "alice123",
                Some("alice@example.com"),
                Some("Alice"),
            ),
            ("bob", "bob123", Some("bob@example.com"), Some("Bob")),
        ];

        for (username, password, email, display_name) in test_users {
            match identity_provider
                .add_user(
                    username.to_string(),
                    password.to_string(),
                    email.map(|s| s.to_string()),
                    display_name.map(|s| s.to_string()),
                )
                .await
            {
                Ok(_) => debug!("Created test user: {}", username),
                Err(e) => debug!("Test user {} might already exist: {}", username, e),
            }
        }
    }

    // Create session service from configuration
    let session_config = SessionConfig {
        jwt_secret: config.auth.jwt_secret.clone(),
        jwt_ttl: chrono::Duration::seconds(config.auth.jwt_ttl_seconds),
        refresh_enabled: config.auth.refresh_enabled,
        enforce_active_sessions: true,
        algorithm: JwtAlgorithm::from_name(&config.auth.jwt_algorithm)
            .unwrap_or(JwtAlgorithm::HS256),
    };
    info!(
        "Creating session service with JWT TTL: {} seconds",
        config.auth.jwt_ttl_seconds
    );
    let session_service = Arc::new(
        SessionService::new(session_config)
            .map_err(anyhow::Error::from)?
            .with_permissions(Arc::new(ChatPermissions::new(config.admin.users.clone()))),
    );

    // Register the identity provider with the session service
    // We need to dereference the Arc and clone the inner provider since register_provider takes Box
    session_service
        .register_provider(Box::new((*identity_provider).clone()))
        .await;

    // Create JWT auth provider
    let auth_provider = Arc::new(JwtAuthProvider::new(session_service.clone()));

    // Create connection manager
    let connection_manager = Arc::new(DefaultConnectionManager::new());

    // Create chat server with configuration
    let chat_server = Arc::new(
        ChatServer::new_with_rate_limit(config.chat.clone(), config.rate_limit.clone())
            .await
            .map_err(|e| {
                error!("Failed to create chat server: {}", e);
                e
            })?,
    );

    // Create handler with the service and connection manager
    let handler = Arc::new(bidirectional_chat_api::ChatServiceHandler::new(
        chat_server.clone(),
        connection_manager.clone(),
    ));

    // Build WebSocket service
    let ws_service = WebSocketServiceBuilder::builder()
        .handler(handler)
        .auth_provider(auth_provider.clone())
        .require_auth(true)
        .build()
        .build_with_manager(connection_manager);

    // Create auth handlers with the shared identity provider
    let auth_handlers = AuthHandlers {
        session_service: session_service.clone(),
        identity_provider: identity_provider.clone(),
    };

    // Build REST service using the macro-generated builder
    // Create auth service implementation
    struct AuthServiceImpl {
        handlers: AuthHandlers,
    }

    #[async_trait::async_trait]
    impl bidirectional_chat_api::auth::ChatAuthServiceTrait for AuthServiceImpl {
        async fn post_auth_login(&self, request: LoginRequest) -> RestResult<LoginResponse> {
            self.handlers.handle_login(request).await
        }

        async fn post_auth_register(
            &self,
            request: RegisterRequest,
        ) -> RestResult<RegisterResponse> {
            self.handlers.handle_register(request).await
        }

        async fn get_health(&self) -> RestResult<HealthResponse> {
            self.handlers.handle_health().await
        }
    }

    let auth_service_impl = AuthServiceImpl {
        handlers: auth_handlers.clone(),
    };

    let auth_router = ChatAuthServiceBuilder::new(auth_service_impl)
        .auth_provider(auth_provider.as_ref().clone())
        .build();

    // Create WebSocket endpoint
    type ChatServiceType = BuiltWebSocketService<
        bidirectional_chat_api::ChatServiceHandler<ChatServer, DefaultConnectionManager>,
        JwtAuthProvider,
        DefaultConnectionManager,
    >;
    let ws_router = Router::new()
        .route("/ws", get(websocket_handler::<ChatServiceType>))
        .with_state(ws_service);

    // Configure CORS based on configuration
    let cors_layer = if config.server.cors.allow_any_origin {
        CorsLayer::permissive()
    } else {
        let mut cors = CorsLayer::new();
        for origin in &config.server.cors.allowed_origins {
            cors = cors.allow_origin(origin.parse::<axum::http::HeaderValue>().unwrap());
        }
        cors
    };

    // Combine all routers
    let app = Router::new()
        .merge(auth_router)
        .merge(ws_router)
        .layer(cors_layer);

    // Start server
    let addr = config.socket_addr();

    info!("Chat server listening on http://{}", addr);
    info!("WebSocket endpoint: ws://{}/ws", addr);
    info!("Health check endpoint: http://{}/health", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        error!("Failed to bind to address {}: {}", addr, e);
        e
    })?;

    info!("Server started successfully, ready to accept connections");

    axum::serve(listener, app).await.map_err(|e| {
        error!("Server error: {}", e);
        e
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ras_jsonrpc_bidirectional_server::MessageHandler;
    use ras_jsonrpc_bidirectional_server::connection::{ChannelMessageSender, ConnectionContext};
    use ras_jsonrpc_bidirectional_server::handler::{
        WebSocketHandler, WebSocketIo, WebSocketIoMessage,
    };
    use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionInfo};
    use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
    use std::collections::VecDeque;
    use std::future;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    struct InMemorySocket {
        incoming: VecDeque<WebSocketIoMessage>,
        outgoing: Vec<WebSocketIoMessage>,
        close_when_empty: bool,
        close_after_outgoing: Option<usize>,
    }

    impl InMemorySocket {
        fn closing_after_outgoing(
            incoming: impl IntoIterator<Item = WebSocketIoMessage>,
            outgoing_count: usize,
        ) -> Self {
            Self {
                incoming: incoming.into_iter().collect(),
                outgoing: Vec::new(),
                close_when_empty: false,
                close_after_outgoing: Some(outgoing_count),
            }
        }
    }

    #[async_trait::async_trait]
    impl WebSocketIo for InMemorySocket {
        async fn send(
            &mut self,
            message: WebSocketIoMessage,
        ) -> ras_jsonrpc_bidirectional_server::ServerResult<()> {
            self.outgoing.push(message);
            if self
                .close_after_outgoing
                .is_some_and(|count| self.outgoing.len() >= count)
            {
                self.close_when_empty = true;
            }
            Ok(())
        }

        async fn recv(
            &mut self,
        ) -> Option<ras_jsonrpc_bidirectional_server::ServerResult<WebSocketIoMessage>> {
            if let Some(message) = self.incoming.pop_front() {
                Some(Ok(message))
            } else if self.close_when_empty {
                None
            } else {
                future::pending().await
            }
        }
    }

    async fn test_chat_server(temp_dir: &TempDir) -> Result<Arc<ChatServer>> {
        test_chat_server_with_rate_limit(temp_dir, config::RateLimitConfig::default()).await
    }

    async fn test_chat_server_with_rate_limit(
        temp_dir: &TempDir,
        rate_limit: config::RateLimitConfig,
    ) -> Result<Arc<ChatServer>> {
        let chat_config = config::ChatConfig {
            data_dir: temp_dir.path().join("chat_data"),
            ..Default::default()
        };

        Ok(Arc::new(
            ChatServer::new_with_rate_limit(chat_config, rate_limit).await?,
        ))
    }

    fn test_user(username: &str, permissions: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: username.to_string(),
            permissions: permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect(),
            metadata: Default::default(),
        }
    }

    fn request(id: &str, method: &str, params: serde_json::Value) -> WebSocketIoMessage {
        let request = JsonRpcRequest::new(
            method.to_string(),
            Some(params),
            Some(serde_json::Value::String(id.to_string())),
        );
        let message = BidirectionalMessage::Request(request);
        WebSocketIoMessage::Text(serde_json::to_string(&message).unwrap())
    }

    struct TestConnection {
        context: Arc<ConnectionContext>,
        messages: mpsc::Receiver<BidirectionalMessage>,
        user: AuthenticatedUser,
    }

    async fn register_test_connection(
        connection_manager: &Arc<DefaultConnectionManager>,
        user: AuthenticatedUser,
    ) -> Result<TestConnection> {
        let connection_id = ConnectionId::new();
        let (message_tx, messages) = mpsc::channel(16);
        let sender = ChannelMessageSender::new(connection_id, message_tx);

        let mut info = ConnectionInfo::new(connection_id);
        info.set_user(user.clone());

        let context = Arc::new(ConnectionContext::new(connection_id, sender.clone()));
        context.set_user(user.clone()).await;

        connection_manager
            .add_connection_with_sender(info, Box::new(sender))
            .await?;

        Ok(TestConnection {
            context,
            messages,
            user,
        })
    }

    fn drain_messages(
        receiver: &mut mpsc::Receiver<BidirectionalMessage>,
    ) -> Vec<BidirectionalMessage> {
        let mut messages = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            messages.push(message);
        }
        messages
    }

    async fn call_handler(
        handler: &ChatServiceHandler<ChatServer, DefaultConnectionManager>,
        context: Arc<ConnectionContext>,
        id: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<JsonRpcResponse> {
        let request = JsonRpcRequest::new(
            method.to_string(),
            Some(params),
            Some(serde_json::Value::String(id.to_string())),
        );

        let response = handler
            .handle_request(request, context)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .ok_or_else(|| anyhow::anyhow!("handler returned no response for {method}"))?;

        Ok(response)
    }

    async fn run_socketless_chat_flow(
        chat_server: Arc<ChatServer>,
        user: AuthenticatedUser,
        incoming: Vec<WebSocketIoMessage>,
        close_after_outgoing: usize,
    ) -> Result<Vec<BidirectionalMessage>> {
        let connection_manager = Arc::new(DefaultConnectionManager::new());
        let handler = Arc::new(ChatServiceHandler::new(
            Arc::clone(&chat_server),
            Arc::clone(&connection_manager),
        ));

        let connection_id = ConnectionId::new();
        let (message_tx, message_rx) = mpsc::channel(16);
        let sender = ChannelMessageSender::new(connection_id, message_tx);

        let mut info = ConnectionInfo::new(connection_id);
        info.set_user(user.clone());

        let context = Arc::new(ConnectionContext::new(connection_id, sender.clone()));
        context.set_user(user).await;

        connection_manager
            .add_connection_with_sender(info, Box::new(sender))
            .await?;

        let mut socket = InMemorySocket::closing_after_outgoing(incoming, close_after_outgoing);

        tokio::time::timeout(
            Duration::from_secs(2),
            WebSocketHandler::new(handler, context, message_rx, 4096).run_with_io(&mut socket),
        )
        .await
        .expect("socketless chat flow should finish")?;

        Ok(socket
            .outgoing
            .into_iter()
            .filter_map(|message| match message {
                WebSocketIoMessage::Text(text) => serde_json::from_str(&text).ok(),
                _ => None,
            })
            .collect())
    }

    fn response_by_id<'a>(
        messages: &'a [BidirectionalMessage],
        id: &str,
    ) -> Option<&'a JsonRpcResponse> {
        messages.iter().find_map(|message| match message {
            BidirectionalMessage::Response(response)
                if response.id.as_ref() == Some(&serde_json::Value::String(id.to_string())) =>
            {
                Some(response)
            }
            _ => None,
        })
    }

    fn notification_by_method<'a>(
        messages: &'a [BidirectionalMessage],
        method: &str,
    ) -> Option<&'a ras_jsonrpc_bidirectional_types::ServerNotification> {
        messages.iter().find_map(|message| match message {
            BidirectionalMessage::ServerNotification(notification)
                if notification.method == method =>
            {
                Some(notification)
            }
            _ => None,
        })
    }

    fn notifications_by_method<'a>(
        messages: &'a [BidirectionalMessage],
        method: &str,
    ) -> Vec<&'a ras_jsonrpc_bidirectional_types::ServerNotification> {
        messages
            .iter()
            .filter_map(|message| match message {
                BidirectionalMessage::ServerNotification(notification)
                    if notification.method == method =>
                {
                    Some(notification)
                }
                _ => None,
            })
            .collect()
    }

    fn room_info<'a>(response: &'a ListRoomsResponse, room_id: &str) -> Option<&'a RoomInfo> {
        response.rooms.iter().find(|room| room.room_id == room_id)
    }

    #[tokio::test]
    async fn websocket_flow_joins_room_and_broadcasts_message_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;

        let messages = run_socketless_chat_flow(
            chat_server,
            test_user("alice", &["user"]),
            vec![
                request("join", "join_room", json!({ "room_name": "general" })),
                request("send", "send_message", json!({ "text": "hello from test" })),
            ],
            7,
        )
        .await?;

        let join_response = response_by_id(&messages, "join").expect("join_room response");
        assert!(
            join_response.error.is_none(),
            "join_room should succeed: {:?}",
            join_response.error
        );
        let join_result: JoinRoomResponse =
            serde_json::from_value(join_response.result.clone().expect("join result"))?;
        assert_eq!(join_result.room_id, "general");
        assert_eq!(join_result.user_count, 1);
        assert!(join_result.existing_users.is_empty());

        let send_response = response_by_id(&messages, "send").expect("send_message response");
        assert!(
            send_response.error.is_none(),
            "send_message should succeed: {:?}",
            send_response.error
        );
        let send_result: SendMessageResponse =
            serde_json::from_value(send_response.result.clone().expect("send result"))?;
        assert_eq!(send_result.message_id, 1);

        let joined = notification_by_method(&messages, "user_joined").expect("join notification");
        let joined: UserJoinedNotification = serde_json::from_value(joined.params.clone())?;
        assert_eq!(joined.username, "alice");
        assert_eq!(joined.room_id, "general");

        let received =
            notification_by_method(&messages, "message_received").expect("message notification");
        let received: MessageReceivedNotification =
            serde_json::from_value(received.params.clone())?;
        assert_eq!(received.username, "alice");
        assert_eq!(received.text, "hello from test");
        assert_eq!(received.room_id, "general");

        Ok(())
    }

    #[tokio::test]
    async fn multi_user_broadcast_reaches_all_room_members_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;
        let connection_manager = Arc::new(DefaultConnectionManager::new());
        let handler =
            ChatServiceHandler::new(Arc::clone(&chat_server), Arc::clone(&connection_manager));

        let mut alice =
            register_test_connection(&connection_manager, test_user("alice", &["user"])).await?;
        let mut bob =
            register_test_connection(&connection_manager, test_user("bob", &["user"])).await?;

        handler
            .on_client_authenticated(alice.context.id, &alice.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        handler
            .on_client_authenticated(bob.context.id, &bob.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        drain_messages(&mut alice.messages);
        drain_messages(&mut bob.messages);

        let alice_join = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "alice-join",
            "join_room",
            json!({ "room_name": "general" }),
        )
        .await?;
        assert!(alice_join.error.is_none());

        let bob_join = call_handler(
            &handler,
            Arc::clone(&bob.context),
            "bob-join",
            "join_room",
            json!({ "room_name": "general" }),
        )
        .await?;
        assert!(bob_join.error.is_none());
        let bob_join: JoinRoomResponse =
            serde_json::from_value(bob_join.result.expect("bob join result"))?;
        assert_eq!(bob_join.existing_users, vec!["alice".to_string()]);
        assert_eq!(bob_join.user_count, 2);

        drain_messages(&mut alice.messages);
        drain_messages(&mut bob.messages);

        let send_response = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "alice-send",
            "send_message",
            json!({ "text": "hello bob" }),
        )
        .await?;
        assert!(
            send_response.error.is_none(),
            "send_message should succeed: {:?}",
            send_response.error
        );

        let alice_messages = drain_messages(&mut alice.messages);
        let bob_messages = drain_messages(&mut bob.messages);

        for (username, messages) in [
            ("alice", alice_messages.as_slice()),
            ("bob", bob_messages.as_slice()),
        ] {
            let notifications = notifications_by_method(messages, "message_received");
            assert_eq!(
                notifications.len(),
                1,
                "{username} should receive one message notification"
            );
            let notification: MessageReceivedNotification =
                serde_json::from_value(notifications[0].params.clone())?;
            assert_eq!(notification.username, "alice");
            assert_eq!(notification.text, "hello bob");
            assert_eq!(notification.room_id, "general");
        }

        Ok(())
    }

    #[tokio::test]
    async fn multi_user_room_list_and_leave_update_presence_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;
        let connection_manager = Arc::new(DefaultConnectionManager::new());
        let handler =
            ChatServiceHandler::new(Arc::clone(&chat_server), Arc::clone(&connection_manager));

        let mut alice =
            register_test_connection(&connection_manager, test_user("alice", &["user"])).await?;
        let mut bob =
            register_test_connection(&connection_manager, test_user("bob", &["user"])).await?;

        handler
            .on_client_authenticated(alice.context.id, &alice.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        handler
            .on_client_authenticated(bob.context.id, &bob.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        drain_messages(&mut alice.messages);
        drain_messages(&mut bob.messages);

        let alice_join = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "alice-join",
            "join_room",
            json!({ "room_name": "general" }),
        )
        .await?;
        assert!(alice_join.error.is_none());

        let bob_join = call_handler(
            &handler,
            Arc::clone(&bob.context),
            "bob-join",
            "join_room",
            json!({ "room_name": "general" }),
        )
        .await?;
        assert!(bob_join.error.is_none());

        drain_messages(&mut alice.messages);
        drain_messages(&mut bob.messages);

        let before_leave = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "list-before-leave",
            "list_rooms",
            json!({}),
        )
        .await?;
        assert!(before_leave.error.is_none());
        let before_leave: ListRoomsResponse =
            serde_json::from_value(before_leave.result.expect("list before leave result"))?;
        let general = room_info(&before_leave, "general").expect("general room before leave");
        assert_eq!(general.user_count, 2);

        let bob_leave = call_handler(
            &handler,
            Arc::clone(&bob.context),
            "bob-leave",
            "leave_room",
            json!({ "room_id": "general" }),
        )
        .await?;
        assert!(
            bob_leave.error.is_none(),
            "leave_room should succeed: {:?}",
            bob_leave.error
        );

        let alice_messages = drain_messages(&mut alice.messages);
        let left =
            notification_by_method(&alice_messages, "user_left").expect("user_left notification");
        let left: UserLeftNotification = serde_json::from_value(left.params.clone())?;
        assert_eq!(left.username, "bob");
        assert_eq!(left.room_id, "general");
        assert_eq!(left.user_count, 1);

        let after_leave = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "list-after-leave",
            "list_rooms",
            json!({}),
        )
        .await?;
        assert!(after_leave.error.is_none());
        let after_leave: ListRoomsResponse =
            serde_json::from_value(after_leave.result.expect("list after leave result"))?;
        let general = room_info(&after_leave, "general").expect("general room after leave");
        assert_eq!(general.user_count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn profile_update_round_trips_multi_word_avatar_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;
        let connection_manager = Arc::new(DefaultConnectionManager::new());
        let handler =
            ChatServiceHandler::new(Arc::clone(&chat_server), Arc::clone(&connection_manager));

        let mut alice =
            register_test_connection(&connection_manager, test_user("alice", &["user"])).await?;

        handler
            .on_client_authenticated(alice.context.id, &alice.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        drain_messages(&mut alice.messages);

        let before_update = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "profile-before-update",
            "get_profile",
            json!({ "username": "alice" }),
        )
        .await?;
        assert!(
            before_update.error.is_none(),
            "get_profile should return the default profile: {:?}",
            before_update.error
        );
        let before_update: GetProfileResponse =
            serde_json::from_value(before_update.result.expect("profile before update result"))?;
        assert_eq!(before_update.profile.username, "alice");
        assert!(before_update.profile.display_name.is_none());
        assert!(matches!(
            before_update.profile.avatar.breed,
            CatBreed::Tabby
        ));
        assert!(matches!(
            before_update.profile.avatar.color,
            CatColor::Orange
        ));
        assert!(matches!(
            before_update.profile.avatar.expression,
            CatExpression::Happy
        ));

        let update_response = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "profile-update",
            "update_profile",
            json!({
                "display_name": "Captain Alice",
                "avatar": {
                    "breed": "maine_coon",
                    "color": "lilac",
                    "expression": "curious"
                }
            }),
        )
        .await?;
        assert!(
            update_response.error.is_none(),
            "update_profile should succeed: {:?}",
            update_response.error
        );
        let update_response: UpdateProfileResponse =
            serde_json::from_value(update_response.result.expect("profile update result"))?;
        assert_eq!(
            update_response.profile.display_name.as_deref(),
            Some("Captain Alice")
        );
        assert!(matches!(
            update_response.profile.avatar.breed,
            CatBreed::MaineCoon
        ));
        assert!(matches!(
            update_response.profile.avatar.color,
            CatColor::Lilac
        ));
        assert!(matches!(
            update_response.profile.avatar.expression,
            CatExpression::Curious
        ));

        let after_update = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "profile-after-update",
            "get_profile",
            json!({ "username": "alice" }),
        )
        .await?;
        assert!(
            after_update.error.is_none(),
            "get_profile should read the persisted profile: {:?}",
            after_update.error
        );
        let after_update: GetProfileResponse =
            serde_json::from_value(after_update.result.expect("profile after update result"))?;
        assert_eq!(
            after_update.profile.display_name.as_deref(),
            Some("Captain Alice")
        );
        assert!(matches!(
            after_update.profile.avatar.breed,
            CatBreed::MaineCoon
        ));
        assert!(matches!(after_update.profile.avatar.color, CatColor::Lilac));
        assert!(matches!(
            after_update.profile.avatar.expression,
            CatExpression::Curious
        ));

        Ok(())
    }

    #[tokio::test]
    async fn websocket_request_error_allows_later_request_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;

        let messages = run_socketless_chat_flow(
            chat_server,
            test_user("alice", &["user"]),
            vec![
                request(
                    "send-before-join",
                    "send_message",
                    json!({ "text": "too early" }),
                ),
                request(
                    "join-after-error",
                    "join_room",
                    json!({ "room_name": "general" }),
                ),
            ],
            4,
        )
        .await?;

        let error_response =
            response_by_id(&messages, "send-before-join").expect("send_message error response");
        let error = error_response.error.as_ref().expect("send_message error");
        assert_eq!(error.code, ras_jsonrpc_types::error_codes::INTERNAL_ERROR);
        assert!(error.message.contains("User not in any room"));

        let join_response =
            response_by_id(&messages, "join-after-error").expect("join_room response");
        assert!(
            join_response.error.is_none(),
            "join_room should succeed after a previous request error: {:?}",
            join_response.error
        );
        let join_result: JoinRoomResponse =
            serde_json::from_value(join_response.result.clone().expect("join result"))?;
        assert_eq!(join_result.room_id, "general");
        assert_eq!(join_result.user_count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn message_rate_limit_rejects_excess_messages_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server_with_rate_limit(
            &temp_dir,
            config::RateLimitConfig {
                enabled: true,
                messages_per_minute: 1,
                connections_per_ip: 10,
                login_attempts_per_hour: 10,
            },
        )
        .await?;

        let messages = run_socketless_chat_flow(
            chat_server,
            test_user("alice", &["user"]),
            vec![
                request("join", "join_room", json!({ "room_name": "general" })),
                request("send-1", "send_message", json!({ "text": "first" })),
                request("send-2", "send_message", json!({ "text": "second" })),
                request("list-after-limit", "list_rooms", json!({})),
            ],
            9,
        )
        .await?;

        let first_send = response_by_id(&messages, "send-1").expect("first send response");
        assert!(
            first_send.error.is_none(),
            "first message should pass the rate limit: {:?}",
            first_send.error
        );

        let second_send = response_by_id(&messages, "send-2").expect("second send response");
        let error = second_send.error.as_ref().expect("rate limit error");
        assert_eq!(error.code, ras_jsonrpc_types::error_codes::INTERNAL_ERROR);
        assert!(error.message.contains("Rate limit exceeded"));
        assert!(error.message.contains("1 messages per minute"));

        let after_limit =
            response_by_id(&messages, "list-after-limit").expect("list_rooms after rate limit");
        assert!(
            after_limit.error.is_none(),
            "later requests should continue after rate limit rejection: {:?}",
            after_limit.error
        );
        let rooms: ListRoomsResponse =
            serde_json::from_value(after_limit.result.clone().expect("rooms result"))?;
        let general = room_info(&rooms, "general").expect("general room");
        assert_eq!(general.user_count, 1);

        let delivered = notifications_by_method(&messages, "message_received");
        assert_eq!(delivered.len(), 1);
        let delivered: MessageReceivedNotification =
            serde_json::from_value(delivered[0].params.clone())?;
        assert_eq!(delivered.text, "first");

        Ok(())
    }

    #[tokio::test]
    async fn disconnect_clears_room_and_typing_state_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;
        let connection_manager = Arc::new(DefaultConnectionManager::new());
        let handler =
            ChatServiceHandler::new(Arc::clone(&chat_server), Arc::clone(&connection_manager));

        let mut alice =
            register_test_connection(&connection_manager, test_user("alice", &["user"])).await?;
        let mut bob =
            register_test_connection(&connection_manager, test_user("bob", &["user"])).await?;

        handler
            .on_client_authenticated(alice.context.id, &alice.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        handler
            .on_client_authenticated(bob.context.id, &bob.user)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        drain_messages(&mut alice.messages);
        drain_messages(&mut bob.messages);

        for (id, context) in [
            ("alice-join", Arc::clone(&alice.context)),
            ("bob-join", Arc::clone(&bob.context)),
        ] {
            let join = call_handler(
                &handler,
                context,
                id,
                "join_room",
                json!({ "room_name": "general" }),
            )
            .await?;
            assert!(join.error.is_none(), "{id} should join: {:?}", join.error);
        }

        drain_messages(&mut alice.messages);
        drain_messages(&mut bob.messages);

        let start_typing = call_handler(
            &handler,
            Arc::clone(&bob.context),
            "bob-start-typing",
            "start_typing",
            json!({}),
        )
        .await?;
        assert!(
            start_typing.error.is_none(),
            "start_typing should succeed: {:?}",
            start_typing.error
        );

        let alice_messages = drain_messages(&mut alice.messages);
        let started = notification_by_method(&alice_messages, "user_started_typing")
            .expect("user_started_typing notification");
        let started: UserStartedTypingNotification =
            serde_json::from_value(started.params.clone())?;
        assert_eq!(started.username, "bob");
        assert_eq!(started.room_id, "general");

        handler
            .on_client_disconnected(bob.context.id)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        let alice_messages = drain_messages(&mut alice.messages);
        let stopped = notification_by_method(&alice_messages, "user_stopped_typing")
            .expect("user_stopped_typing notification");
        let stopped: UserStoppedTypingNotification =
            serde_json::from_value(stopped.params.clone())?;
        assert_eq!(stopped.username, "bob");
        assert_eq!(stopped.room_id, "general");

        let left =
            notification_by_method(&alice_messages, "user_left").expect("user_left notification");
        let left: UserLeftNotification = serde_json::from_value(left.params.clone())?;
        assert_eq!(left.username, "bob");
        assert_eq!(left.room_id, "general");
        assert_eq!(left.user_count, 1);

        let after_disconnect = call_handler(
            &handler,
            Arc::clone(&alice.context),
            "list-after-disconnect",
            "list_rooms",
            json!({}),
        )
        .await?;
        assert!(after_disconnect.error.is_none());
        let after_disconnect: ListRoomsResponse = serde_json::from_value(
            after_disconnect
                .result
                .expect("list after disconnect result"),
        )?;
        let general =
            room_info(&after_disconnect, "general").expect("general room after disconnect");
        assert_eq!(general.user_count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn admin_operations_kick_and_broadcast_without_socket() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let chat_server = test_chat_server(&temp_dir).await?;
        let connection_manager = Arc::new(DefaultConnectionManager::new());
        let handler =
            ChatServiceHandler::new(Arc::clone(&chat_server), Arc::clone(&connection_manager));

        let mut admin =
            register_test_connection(&connection_manager, test_user("admin", &["admin", "user"]))
                .await?;
        let mut moderator = register_test_connection(
            &connection_manager,
            test_user("moderator", &["moderator", "user"]),
        )
        .await?;
        let mut bob =
            register_test_connection(&connection_manager, test_user("bob", &["user"])).await?;

        for connection in [&admin, &moderator, &bob] {
            handler
                .on_client_authenticated(connection.context.id, &connection.user)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }

        drain_messages(&mut admin.messages);
        drain_messages(&mut moderator.messages);
        drain_messages(&mut bob.messages);

        let denied_broadcast = call_handler(
            &handler,
            Arc::clone(&bob.context),
            "broadcast-denied",
            "broadcast_announcement",
            json!({ "message": "not allowed", "level": "warning" }),
        )
        .await?;
        let denied = denied_broadcast
            .error
            .as_ref()
            .expect("regular user should not broadcast announcements");
        assert_eq!(denied.code, -32002);

        let bob_join = call_handler(
            &handler,
            Arc::clone(&bob.context),
            "bob-join",
            "join_room",
            json!({ "room_name": "general" }),
        )
        .await?;
        assert!(bob_join.error.is_none());
        drain_messages(&mut bob.messages);

        let kick_response = call_handler(
            &handler,
            Arc::clone(&moderator.context),
            "kick-bob",
            "kick_user",
            json!({ "target_username": "bob", "reason": "policy violation" }),
        )
        .await?;
        assert!(
            kick_response.error.is_none(),
            "kick_user should succeed for moderators: {:?}",
            kick_response.error
        );
        assert_eq!(
            kick_response.result.expect("kick result"),
            serde_json::Value::Bool(true)
        );

        let bob_messages = drain_messages(&mut bob.messages);
        let kicked =
            notification_by_method(&bob_messages, "user_kicked").expect("user_kicked notification");
        let kicked: UserKickedNotification = serde_json::from_value(kicked.params.clone())?;
        assert_eq!(kicked.username, "bob");
        assert_eq!(kicked.reason, "policy violation");
        assert_eq!(kicked.room_id, "general");

        let after_kick = call_handler(
            &handler,
            Arc::clone(&moderator.context),
            "list-after-kick",
            "list_rooms",
            json!({}),
        )
        .await?;
        assert!(after_kick.error.is_none());
        let after_kick: ListRoomsResponse =
            serde_json::from_value(after_kick.result.expect("list after kick result"))?;
        let general = room_info(&after_kick, "general").expect("general room after kick");
        assert_eq!(general.user_count, 0);

        let announcement_response = call_handler(
            &handler,
            Arc::clone(&admin.context),
            "broadcast-announcement",
            "broadcast_announcement",
            json!({ "message": "maintenance soon", "level": "warning" }),
        )
        .await?;
        assert!(
            announcement_response.error.is_none(),
            "broadcast_announcement should succeed for admins: {:?}",
            announcement_response.error
        );

        for (username, messages) in [
            ("admin", drain_messages(&mut admin.messages)),
            ("moderator", drain_messages(&mut moderator.messages)),
        ] {
            let announcement = notification_by_method(&messages, "system_announcement")
                .unwrap_or_else(|| {
                    panic!("{username} should receive system_announcement notification")
                });
            let announcement: SystemAnnouncementNotification =
                serde_json::from_value(announcement.params.clone())?;
            assert_eq!(announcement.message, "maintenance soon");
            assert!(matches!(announcement.level, AnnouncementLevel::Warning));
        }
        assert!(drain_messages(&mut bob.messages).is_empty());

        Ok(())
    }
}
