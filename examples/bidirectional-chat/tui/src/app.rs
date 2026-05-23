use crate::avatar::AvatarManager;
use anyhow::Result;
use bidirectional_chat_api::{
    ChatServiceClient, ChatServiceClientBuilder, JoinRoomRequest, LeaveRoomRequest,
    ListRoomsRequest, MessageReceivedNotification, RoomInfo, SendMessageRequest,
    StartTypingRequest, StopTypingRequest, SystemAnnouncementNotification, UserJoinedNotification,
    UserLeftNotification, UserStartedTypingNotification, UserStoppedTypingNotification,
};
use chrono::{DateTime, Local};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct Message {
    pub username: String,
    pub text: String,
    pub timestamp: DateTime<Local>,
    pub room_id: String,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    MessageReceived(Message),
    UserJoined { username: String, room_id: String },
    UserLeft { username: String, room_id: String },
    UserStartedTyping { username: String, room_id: String },
    UserStoppedTyping { username: String, room_id: String },
    SystemAnnouncement { message: String },
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppScreen {
    Login,
    Register,
    RoomList,
    Chat { room_id: String, room_name: String },
}

pub struct AppState {
    pub screen: AppScreen,
    pub messages: Vec<Message>,
    pub rooms: Vec<RoomInfo>,
    pub current_room: Option<(String, String)>, // (room_id, room_name)
    pub username: Option<String>,
    pub error_message: Option<String>,
    pub input_buffer: String,
    pub auth_username_input: String,
    pub auth_password_input: String,
    pub auth_field_focus: AuthField,
    pub connected: bool,
    pub avatar_manager: AvatarManager,
    pub room_users: std::collections::HashMap<String, Vec<String>>, // room_id -> list of users
    pub typing_users: std::collections::HashMap<String, std::collections::HashSet<String>>, // room_id -> set of typing users
    pub last_typing_time: Option<std::time::Instant>,
    pub is_typing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthField {
    Username,
    Password,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            screen: AppScreen::Login,
            messages: Vec::new(),
            rooms: Vec::new(),
            current_room: None,
            username: None,
            error_message: None,
            input_buffer: String::new(),
            auth_username_input: String::new(),
            auth_password_input: String::new(),
            auth_field_focus: AuthField::Username,
            connected: false,
            avatar_manager: AvatarManager::new(),
            room_users: std::collections::HashMap::new(),
            typing_users: std::collections::HashMap::new(),
            last_typing_time: None,
            is_typing: false,
        }
    }
}

impl AppState {
    pub fn enter_room(&mut self, room_id: String, room_name: String, existing_users: Vec<String>) {
        self.current_room = Some((room_id.clone(), room_name.clone()));
        self.screen = AppScreen::Chat { room_id, room_name };
        self.messages.clear();

        let Some((room_id, _)) = &self.current_room else {
            return;
        };

        let mut users = Vec::new();
        for user in existing_users {
            if !users.contains(&user) {
                users.push(user);
            }
        }

        if let Some(username) = &self.username
            && !users.contains(username)
        {
            users.push(username.clone());
        }

        self.room_users.insert(room_id.clone(), users);
    }

    pub fn leave_room(&mut self, room_id: &str) {
        self.screen = AppScreen::RoomList;
        self.current_room = None;
        self.input_buffer.clear();
        self.room_users.remove(room_id);
        self.typing_users.remove(room_id);
    }

    pub fn apply_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::MessageReceived(message) => {
                self.messages.push(message);
            }
            AppEvent::UserJoined { username, room_id } => {
                let users = self.room_users.entry(room_id.clone()).or_default();
                if !users.contains(&username) {
                    users.push(username.clone());
                }

                self.push_system_message(format!("{} joined the room", username), room_id);
            }
            AppEvent::UserLeft { username, room_id } => {
                if let Some(users) = self.room_users.get_mut(&room_id) {
                    users.retain(|user| user != &username);
                }

                self.push_system_message(format!("{} left the room", username), room_id);
            }
            AppEvent::SystemAnnouncement { message } => {
                if let Some((room_id, _)) = &self.current_room {
                    self.push_system_message(message, room_id.clone());
                }
            }
            AppEvent::Connected => {
                self.connected = true;
            }
            AppEvent::Disconnected => {
                self.connected = false;
                self.screen = AppScreen::Login;
                self.error_message = Some("Disconnected from server".to_string());
            }
            AppEvent::UserStartedTyping { username, room_id } => {
                self.typing_users
                    .entry(room_id)
                    .or_default()
                    .insert(username);
            }
            AppEvent::UserStoppedTyping { username, room_id } => {
                if let Some(typing_users) = self.typing_users.get_mut(&room_id) {
                    typing_users.remove(&username);
                    if typing_users.is_empty() {
                        self.typing_users.remove(&room_id);
                    }
                }
            }
        }
    }

    fn push_system_message(&mut self, text: String, room_id: String) {
        self.messages.push(Message {
            username: "System".to_string(),
            text,
            timestamp: Local::now(),
            room_id,
        });
    }
}

pub struct ChatClient {
    client: Option<ChatServiceClient>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl ChatClient {
    pub fn new(event_tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        Self {
            client: None,
            event_tx,
        }
    }

    pub async fn connect(&mut self, server_url: &str, jwt_token: String) -> Result<()> {
        let ws_url = format!(
            "{}/ws",
            server_url
                .replace("http://", "ws://")
                .replace("https://", "wss://")
        );

        let mut client = ChatServiceClientBuilder::new(ws_url)
            .with_jwt_token(jwt_token)
            .build()
            .await?;

        self.setup_event_handlers(&mut client);

        client.connect().await?;

        self.client = Some(client);
        self.event_tx.send(AppEvent::Connected)?;

        Ok(())
    }

    fn setup_event_handlers(&self, client: &mut ChatServiceClient) {
        let tx = self.event_tx.clone();
        client.on_message_received(move |notification: MessageReceivedNotification| {
            let message = Message {
                username: notification.username,
                text: notification.text,
                timestamp: DateTime::parse_from_rfc3339(&notification.timestamp)
                    .unwrap_or_else(|_| Local::now().into())
                    .with_timezone(&Local),
                room_id: notification.room_id,
            };
            let _ = tx.send(AppEvent::MessageReceived(message));
        });

        let tx = self.event_tx.clone();
        client.on_user_joined(move |notification: UserJoinedNotification| {
            let _ = tx.send(AppEvent::UserJoined {
                username: notification.username,
                room_id: notification.room_id,
            });
        });

        let tx = self.event_tx.clone();
        client.on_user_left(move |notification: UserLeftNotification| {
            let _ = tx.send(AppEvent::UserLeft {
                username: notification.username,
                room_id: notification.room_id,
            });
        });

        let tx = self.event_tx.clone();
        client.on_system_announcement(move |notification: SystemAnnouncementNotification| {
            let _ = tx.send(AppEvent::SystemAnnouncement {
                message: notification.message,
            });
        });

        let tx = self.event_tx.clone();
        client.on_user_started_typing(move |notification: UserStartedTypingNotification| {
            let _ = tx.send(AppEvent::UserStartedTyping {
                username: notification.username,
                room_id: notification.room_id,
            });
        });

        let tx = self.event_tx.clone();
        client.on_user_stopped_typing(move |notification: UserStoppedTypingNotification| {
            let _ = tx.send(AppEvent::UserStoppedTyping {
                username: notification.username,
                room_id: notification.room_id,
            });
        });
    }

    pub async fn list_rooms(&self) -> Result<Vec<RoomInfo>> {
        match &self.client {
            Some(client) => {
                let response = client.list_rooms(ListRoomsRequest {}).await?;
                Ok(response.rooms)
            }
            None => anyhow::bail!("Not connected"),
        }
    }

    pub async fn join_room(&self, room_name: String) -> Result<(String, Vec<String>)> {
        match &self.client {
            Some(client) => {
                let response = client
                    .join_room(JoinRoomRequest {
                        room_name: room_name.clone(),
                    })
                    .await?;
                Ok((response.room_id, response.existing_users))
            }
            None => anyhow::bail!("Not connected"),
        }
    }

    pub async fn leave_room(&self, room_id: String) -> Result<()> {
        match &self.client {
            Some(client) => {
                client.leave_room(LeaveRoomRequest { room_id }).await?;
                Ok(())
            }
            None => anyhow::bail!("Not connected"),
        }
    }

    pub async fn send_message(&self, text: String) -> Result<()> {
        match &self.client {
            Some(client) => {
                client.send_message(SendMessageRequest { text }).await?;
                Ok(())
            }
            None => anyhow::bail!("Not connected"),
        }
    }

    pub async fn start_typing(&self) -> Result<()> {
        match &self.client {
            Some(client) => {
                client.start_typing(StartTypingRequest {}).await?;
                Ok(())
            }
            None => anyhow::bail!("Not connected"),
        }
    }

    pub async fn stop_typing(&self) -> Result<()> {
        match &self.client {
            Some(client) => {
                client.stop_typing(StopTypingRequest {}).await?;
                Ok(())
            }
            None => anyhow::bail!("Not connected"),
        }
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(client) = self.client.take() {
            client.disconnect().await?;
            self.event_tx.send(AppEvent::Disconnected)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(room_id: &str, text: &str) -> Message {
        Message {
            username: "alice".to_string(),
            text: text.to_string(),
            timestamp: Local::now(),
            room_id: room_id.to_string(),
        }
    }

    #[test]
    fn default_state_starts_on_login_screen() {
        let app = AppState::default();

        assert_eq!(app.screen, AppScreen::Login);
        assert!(!app.connected);
        assert!(app.messages.is_empty());
        assert_eq!(app.auth_field_focus, AuthField::Username);
    }

    #[test]
    fn enter_room_tracks_current_room_and_deduplicates_users() {
        let mut app = AppState {
            username: Some("alice".to_string()),
            messages: vec![message("lobby", "stale")],
            ..AppState::default()
        };

        app.enter_room(
            "room-1".to_string(),
            "General".to_string(),
            vec!["bob".to_string(), "bob".to_string(), "alice".to_string()],
        );

        assert_eq!(
            app.screen,
            AppScreen::Chat {
                room_id: "room-1".to_string(),
                room_name: "General".to_string(),
            }
        );
        assert_eq!(
            app.current_room,
            Some(("room-1".to_string(), "General".to_string()))
        );
        assert!(app.messages.is_empty());
        assert_eq!(
            app.room_users.get("room-1").expect("room users"),
            &vec!["bob".to_string(), "alice".to_string()]
        );
    }

    #[test]
    fn leave_room_clears_chat_state_for_that_room() {
        let mut app = AppState {
            current_room: Some(("room-1".to_string(), "General".to_string())),
            screen: AppScreen::Chat {
                room_id: "room-1".to_string(),
                room_name: "General".to_string(),
            },
            input_buffer: "draft".to_string(),
            ..AppState::default()
        };
        app.room_users
            .insert("room-1".to_string(), vec!["alice".to_string()]);

        app.leave_room("room-1");

        assert_eq!(app.screen, AppScreen::RoomList);
        assert_eq!(app.current_room, None);
        assert!(app.input_buffer.is_empty());
        assert!(!app.room_users.contains_key("room-1"));
    }

    #[test]
    fn apply_message_and_system_announcement_append_room_messages() {
        let mut app = AppState {
            current_room: Some(("room-1".to_string(), "General".to_string())),
            ..AppState::default()
        };

        app.apply_event(AppEvent::MessageReceived(message("room-1", "hello")));
        app.apply_event(AppEvent::SystemAnnouncement {
            message: "maintenance soon".to_string(),
        });

        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].text, "hello");
        assert_eq!(app.messages[1].username, "System");
        assert_eq!(app.messages[1].text, "maintenance soon");
        assert_eq!(app.messages[1].room_id, "room-1");
    }

    #[test]
    fn apply_user_joined_deduplicates_user_and_records_notice() {
        let mut app = AppState::default();
        app.room_users
            .insert("room-1".to_string(), vec!["alice".to_string()]);

        app.apply_event(AppEvent::UserJoined {
            username: "alice".to_string(),
            room_id: "room-1".to_string(),
        });

        assert_eq!(
            app.room_users.get("room-1").expect("room users"),
            &vec!["alice".to_string()]
        );
        assert_eq!(app.messages[0].text, "alice joined the room");
    }

    #[test]
    fn apply_user_left_removes_user_and_records_notice() {
        let mut app = AppState::default();
        app.room_users.insert(
            "room-1".to_string(),
            vec!["alice".to_string(), "bob".to_string()],
        );

        app.apply_event(AppEvent::UserLeft {
            username: "alice".to_string(),
            room_id: "room-1".to_string(),
        });

        assert_eq!(
            app.room_users.get("room-1").expect("room users"),
            &vec!["bob".to_string()]
        );
        assert_eq!(app.messages[0].text, "alice left the room");
    }

    #[test]
    fn typing_events_remove_empty_room_sets() {
        let mut app = AppState::default();

        app.apply_event(AppEvent::UserStartedTyping {
            username: "alice".to_string(),
            room_id: "room-1".to_string(),
        });
        assert!(
            app.typing_users
                .get("room-1")
                .expect("typing users")
                .contains("alice")
        );

        app.apply_event(AppEvent::UserStoppedTyping {
            username: "alice".to_string(),
            room_id: "room-1".to_string(),
        });
        assert!(!app.typing_users.contains_key("room-1"));
    }

    #[test]
    fn disconnected_event_returns_to_login_with_error() {
        let mut app = AppState {
            connected: true,
            screen: AppScreen::RoomList,
            ..AppState::default()
        };

        app.apply_event(AppEvent::Disconnected);

        assert!(!app.connected);
        assert_eq!(app.screen, AppScreen::Login);
        assert_eq!(
            app.error_message.as_deref(),
            Some("Disconnected from server")
        );
    }
}
