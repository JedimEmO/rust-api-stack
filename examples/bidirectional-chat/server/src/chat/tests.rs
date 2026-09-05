use super::*;
use ras_jsonrpc_bidirectional_server::DefaultConnectionManager;
use ras_jsonrpc_bidirectional_server::MessageHandler;
use ras_jsonrpc_bidirectional_server::connection::{ChannelMessageSender, ConnectionContext};
use ras_jsonrpc_bidirectional_server::handler::{
    WebSocketHandler, WebSocketIo, WebSocketIoMessage,
};
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionInfo};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::json;
use std::collections::VecDeque;
use std::future;
use std::time::Duration;
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
    messages: mpsc::Receiver<ras_jsonrpc_bidirectional_server::OutboundMessage>,
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
    receiver: &mut mpsc::Receiver<ras_jsonrpc_bidirectional_server::OutboundMessage>,
) -> Vec<BidirectionalMessage> {
    let mut messages = Vec::new();
    while let Ok(outbound) = receiver.try_recv() {
        messages.push(outbound.message);
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
        BidirectionalMessage::ServerNotification(notification) if notification.method == method => {
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
    let received: MessageReceivedNotification = serde_json::from_value(received.params.clone())?;
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
    // Handler errors expose a generic message; details stay in server logs.
    assert_eq!(error.message, "Internal error");

    let join_response = response_by_id(&messages, "join-after-error").expect("join_room response");
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
    // The rate-limit reason stays in server logs.
    assert_eq!(error.message, "Internal error");

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
    let started: UserStartedTypingNotification = serde_json::from_value(started.params.clone())?;
    assert_eq!(started.username, "bob");
    assert_eq!(started.room_id, "general");

    handler
        .on_client_disconnected(bob.context.id)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let alice_messages = drain_messages(&mut alice.messages);
    let stopped = notification_by_method(&alice_messages, "user_stopped_typing")
        .expect("user_stopped_typing notification");
    let stopped: UserStoppedTypingNotification = serde_json::from_value(stopped.params.clone())?;
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
    let general = room_info(&after_disconnect, "general").expect("general room after disconnect");
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
        let announcement =
            notification_by_method(&messages, "system_announcement").unwrap_or_else(|| {
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
