//! Persistence layer for chat state
//!
//! This module handles saving and loading chat room state and message history
//! to/from disk. Uses JSON files for simplicity.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use tokio::fs;
use tracing::{debug, error, info, instrument, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRoom {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub users: HashSet<String>, // Current users (for recovery after restart)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedMessage {
    pub id: u64,
    pub room_id: String,
    pub username: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUserProfile {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar: PersistedCatAvatar,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedCatAvatar {
    pub breed: String,
    pub color: String,
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub rooms: HashMap<String, PersistedRoom>,
    pub messages: Vec<PersistedMessage>,
    pub next_message_id: u64,
    pub user_profiles: HashMap<String, PersistedUserProfile>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            rooms: HashMap::new(),
            messages: Vec::new(),
            next_message_id: 1,
            user_profiles: HashMap::new(),
        }
    }
}

pub struct PersistenceManager {
    data_dir: PathBuf,
    state_file: PathBuf,
    messages_dir: PathBuf,
}

impl PersistenceManager {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        let state_file = data_dir.join("chat_state.json");
        let messages_dir = data_dir.join("messages");

        Self {
            data_dir,
            state_file,
            messages_dir,
        }
    }

    #[instrument(skip(self))]
    pub async fn init(&self) -> Result<()> {
        info!(data_dir = ?self.data_dir, "Initializing persistence manager");

        // Create directories if they don't exist
        fs::create_dir_all(&self.data_dir).await.map_err(|e| {
            error!("Failed to create data directory: {}", e);
            e
        })?;
        debug!("Created data directory");

        fs::create_dir_all(&self.messages_dir).await.map_err(|e| {
            error!("Failed to create messages directory: {}", e);
            e
        })?;
        debug!("Created messages directory");

        info!("Persistence manager initialized successfully");
        Ok(())
    }

    #[instrument(skip(self, state), fields(rooms = state.rooms.len(), profiles = state.user_profiles.len()))]
    pub async fn save_state(&self, state: &PersistedState) -> Result<()> {
        debug!("Saving state to disk");

        let json = serde_json::to_string_pretty(state).map_err(|e| {
            error!("Failed to serialize state: {}", e);
            e
        })?;

        fs::write(&self.state_file, json).await.map_err(|e| {
            error!(file = ?self.state_file, "Failed to write state file: {}", e);
            e
        })?;

        debug!("State saved successfully");
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn load_state(&self) -> Result<PersistedState> {
        if !self.state_file.exists() {
            info!("No existing state file found, returning default state");
            return Ok(PersistedState::default());
        }

        debug!(file = ?self.state_file, "Loading state from file");
        let json = fs::read_to_string(&self.state_file).await.map_err(|e| {
            error!("Failed to read state file: {}", e);
            e
        })?;

        let state: PersistedState = serde_json::from_str(&json).map_err(|e| {
            error!("Failed to deserialize state: {}", e);
            e
        })?;

        info!(
            rooms = state.rooms.len(),
            profiles = state.user_profiles.len(),
            messages = state.messages.len(),
            "State loaded successfully"
        );
        Ok(state)
    }

    #[instrument(skip(self, message), fields(room_id = %room_id, message_id = message.id, username = %message.username))]
    pub async fn append_message(&self, room_id: &str, message: &PersistedMessage) -> Result<()> {
        // Save messages per room in separate files for better performance
        let message_file = self.messages_dir.join(format!("{}.jsonl", room_id));
        debug!(file = ?message_file, "Appending message to file");

        let json = serde_json::to_string(message).map_err(|e| {
            error!("Failed to serialize message: {}", e);
            e
        })?;
        let mut content = json;
        content.push('\n');

        // Append to file
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&message_file)
            .await
            .map_err(|e| {
                error!("Failed to open message file: {}", e);
                e
            })?;

        file.write_all(content.as_bytes()).await.map_err(|e| {
            error!("Failed to write message: {}", e);
            e
        })?;

        file.flush().await.map_err(|e| {
            error!("Failed to flush message file: {}", e);
            e
        })?;

        debug!("Message persisted successfully");
        Ok(())
    }

    #[instrument(skip(self), fields(room_id = %room_id, limit = ?limit))]
    pub async fn load_room_messages(
        &self,
        room_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<PersistedMessage>> {
        let message_file = self.messages_dir.join(format!("{}.jsonl", room_id));

        if !message_file.exists() {
            debug!("No message file exists for room {}", room_id);
            return Ok(Vec::new());
        }

        debug!(file = ?message_file, "Loading messages from file");
        let content = fs::read_to_string(&message_file).await.map_err(|e| {
            error!("Failed to read message file: {}", e);
            e
        })?;

        let mut messages: Vec<PersistedMessage> = Vec::new();
        let mut parse_errors = 0;

        for (line_num, line) in content.lines().enumerate() {
            if !line.trim().is_empty() {
                match serde_json::from_str::<PersistedMessage>(line) {
                    Ok(msg) => messages.push(msg),
                    Err(e) => {
                        warn!(line_num = line_num + 1, "Failed to parse message: {}", e);
                        parse_errors += 1;
                    }
                }
            }
        }

        if parse_errors > 0 {
            warn!(
                "Encountered {} parse errors while loading messages",
                parse_errors
            );
        }

        // Apply limit if specified (return most recent messages)
        if let Some(limit) = limit {
            let total_messages = messages.len();
            let start = messages.len().saturating_sub(limit);
            messages = messages[start..].to_vec();
            debug!(
                total_messages,
                returned_messages = messages.len(),
                "Applied message limit"
            );
        }

        info!(room_id = %room_id, message_count = messages.len(), "Messages loaded successfully");
        Ok(messages)
    }

    #[instrument(skip(self), fields(room_id = %room_id))]
    pub async fn delete_room_messages(&self, room_id: &str) -> Result<()> {
        let message_file = self.messages_dir.join(format!("{}.jsonl", room_id));

        if message_file.exists() {
            info!("Deleting message file for room {}", room_id);
            fs::remove_file(&message_file).await.map_err(|e| {
                error!(file = ?message_file, "Failed to delete message file: {}", e);
                e
            })?;
            info!("Message file deleted successfully");
        } else {
            debug!("No message file to delete for room {}", room_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_persistence_manager() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let persistence = PersistenceManager::new(temp_dir.path());
        persistence.init().await?;

        // Test saving and loading state
        let mut state = PersistedState::default();
        let mut room = PersistedRoom {
            id: "general".to_string(),
            name: "General".to_string(),
            created_at: Utc::now(),
            users: HashSet::new(),
        };
        room.users.insert("alice".to_string());
        state.rooms.insert("general".to_string(), room);
        state.next_message_id = 42;

        persistence.save_state(&state).await?;
        let loaded_state = persistence.load_state().await?;

        assert_eq!(loaded_state.next_message_id, 42);
        assert!(loaded_state.rooms.contains_key("general"));

        // Test message persistence
        let msg = PersistedMessage {
            id: 1,
            room_id: "general".to_string(),
            username: "alice".to_string(),
            text: "Hello, world!".to_string(),
            timestamp: Utc::now(),
        };

        persistence.append_message("general", &msg).await?;
        let messages = persistence.load_room_messages("general", None).await?;

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Hello, world!");

        Ok(())
    }

    #[tokio::test]
    async fn load_room_messages_returns_recent_limit_without_underflow() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let persistence = PersistenceManager::new(temp_dir.path());
        persistence.init().await?;

        for id in 1..=3 {
            persistence
                .append_message(
                    "general",
                    &PersistedMessage {
                        id,
                        room_id: "general".to_string(),
                        username: "alice".to_string(),
                        text: format!("message-{id}"),
                        timestamp: Utc::now(),
                    },
                )
                .await?;
        }

        let messages = persistence.load_room_messages("general", Some(1)).await?;

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, 3);
        assert_eq!(messages[0].text, "message-3");

        Ok(())
    }
}
