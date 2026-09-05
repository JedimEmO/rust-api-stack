use crate::config;
use crate::persistence::{
    PersistedCatAvatar, PersistedMessage, PersistedRoom, PersistedUserProfile, PersistenceManager,
};
use anyhow::Result;
use bidirectional_chat_api::*;
use chrono::Utc;
use conversions::*;
use dashmap::DashMap;
use ras_auth_core::AuthenticatedUser;
use ras_jsonrpc_bidirectional_types::{ConnectionId, ConnectionManager};
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

mod conversions;
mod messages;
mod profiles;
mod rooms;
mod service;
mod typing;

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

#[derive(Clone)]
pub(crate) struct ChatServer {
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
    pub(crate) async fn new_with_rate_limit(
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
#[cfg(test)]
mod tests;
