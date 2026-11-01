# Bidirectional chat terminal client

A Ratatui terminal client for the [bidirectional chat example](../README.md).
It signs in over REST, then talks to the server through the `ChatServiceClient`
that `jsonrpc_bidirectional_service!` generates in the [chat API crate](../api/README.md).

## Run

Start the [server](../server/README.md) first, then run the client in another terminal:

```bash
cargo run -p bidirectional-chat-server --locked
cargo run -p bidirectional-chat-tui --locked
```

The client connects to `http://localhost:3000` and opens the WebSocket at `/ws` on the same host.
Set `SERVER_URL`, in the environment or in a `.env` file in the current directory, to use another server:

```env
SERVER_URL=http://127.0.0.1:4000
```

## Accounts

Debug builds of the server create `alice` / `alice123` and `bob` / `bob123`.
Press Ctrl+R on the login screen to register a new account.
When the server runs with `config.example.toml`, `admin` / `admin123456` and `moderator` / `moderator123` also exist.

## Keys

Login and registration screens:

- Tab switches between the username and password fields.
- Enter signs in, or registers and then signs in.
- Ctrl+R opens the registration screen.
- Esc quits from the login screen and goes back from the registration screen.

Room list:

- 1 to 9 join the room with that number.
- R refreshes the list.
- Q disconnects and quits.

Chat:

- Enter sends the message.
- Esc leaves the room and returns to the room list.
- `/quit` or `/exit` disconnects and quits.

The sidebar shows the users in the room with their cat avatars.
When someone types, their avatar shows a speech bubble and a line at the bottom names them;
both clear after five seconds without a keystroke.

## How it uses the generated client

[src/app.rs](src/app.rs) builds the client with `ChatServiceClientBuilder`, passing the JWT from the login response,
and registers a callback for each server notification, such as `on_message_received`.
Each callback forwards the notification to the UI loop over a channel.
Method calls, such as `join_room` and `send_message`, are ordinary async calls with typed requests and responses.
Login and registration in [src/auth.rs](src/auth.rs) use `reqwest` with the request and response types from the API crate.

## Checks

```bash
cargo test -p bidirectional-chat-tui --locked
```
