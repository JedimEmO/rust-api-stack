use crate::app::{AppScreen, AppState, AuthField};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

pub fn draw(frame: &mut Frame, app: &mut AppState) {
    let screen = app.screen.clone();
    match &screen {
        AppScreen::Login => draw_login_screen(frame, app),
        AppScreen::Register => draw_register_screen(frame, app),
        AppScreen::RoomList => draw_room_list_screen(frame, app),
        AppScreen::Chat { room_name, .. } => draw_chat_screen(frame, app, room_name),
    }

    // Draw error popup if there's an error
    if let Some(error) = &app.error_message {
        draw_error_popup(frame, error);
    }
}

fn draw_login_screen(frame: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(frame.area());

    let auth_block = Block::default()
        .title(" Login ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Cyan));

    frame.render_widget(auth_block.clone(), chunks[1]);

    let inner_area = auth_block.inner(chunks[1]);
    let auth_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // Username field
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Password field
            Constraint::Length(1), // Spacing
            Constraint::Min(0),    // Instructions
        ])
        .split(inner_area);

    // Username field
    let username_style = if app.auth_field_focus == AuthField::Username {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let username_field =
        Paragraph::new(format!("Username: {}", app.auth_username_input)).style(username_style);
    frame.render_widget(username_field, auth_chunks[0]);

    // Add underline for username field
    let username_underline = Paragraph::new("─".repeat(auth_chunks[0].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(
        username_underline,
        Rect {
            x: auth_chunks[0].x,
            y: auth_chunks[0].y + 1,
            width: auth_chunks[0].width,
            height: 1,
        },
    );

    // Password field
    let password_style = if app.auth_field_focus == AuthField::Password {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let password_display = "*".repeat(app.auth_password_input.len());
    let password_field =
        Paragraph::new(format!("Password: {}", password_display)).style(password_style);
    frame.render_widget(password_field, auth_chunks[2]);

    // Add underline for password field
    let password_underline = Paragraph::new("─".repeat(auth_chunks[2].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(
        password_underline,
        Rect {
            x: auth_chunks[2].x,
            y: auth_chunks[2].y + 1,
            width: auth_chunks[2].width,
            height: 1,
        },
    );

    // Instructions
    let instructions = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Press "),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to switch fields | "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to login"),
        ]),
        Line::from(vec![
            Span::raw("Press "),
            Span::styled(
                "Ctrl+R",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to create a new account | "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to quit"),
        ]),
    ])
    .style(Style::default().fg(Color::Gray))
    .alignment(Alignment::Center);
    frame.render_widget(instructions, auth_chunks[4]);

    // Set cursor position
    match app.auth_field_focus {
        AuthField::Username => {
            frame.set_cursor_position((
                auth_chunks[0].x + 10 + app.auth_username_input.len() as u16,
                auth_chunks[0].y,
            ));
        }
        AuthField::Password => {
            frame.set_cursor_position((
                auth_chunks[2].x + 10 + app.auth_password_input.len() as u16,
                auth_chunks[2].y,
            ));
        }
    }
}

fn draw_register_screen(frame: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(frame.area());

    let auth_block = Block::default()
        .title(" Create New Account ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Green));

    frame.render_widget(auth_block.clone(), chunks[1]);

    let inner_area = auth_block.inner(chunks[1]);
    let auth_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // Username field
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Password field
            Constraint::Length(1), // Spacing
            Constraint::Min(0),    // Instructions
        ])
        .split(inner_area);

    // Username field
    let username_style = if app.auth_field_focus == AuthField::Username {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let username_field =
        Paragraph::new(format!("Username: {}", app.auth_username_input)).style(username_style);
    frame.render_widget(username_field, auth_chunks[0]);

    // Add underline for username field
    let username_underline = Paragraph::new("─".repeat(auth_chunks[0].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(
        username_underline,
        Rect {
            x: auth_chunks[0].x,
            y: auth_chunks[0].y + 1,
            width: auth_chunks[0].width,
            height: 1,
        },
    );

    // Password field
    let password_style = if app.auth_field_focus == AuthField::Password {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let password_display = "*".repeat(app.auth_password_input.len());
    let password_field =
        Paragraph::new(format!("Password: {}", password_display)).style(password_style);
    frame.render_widget(password_field, auth_chunks[2]);

    // Add underline for password field
    let password_underline = Paragraph::new("─".repeat(auth_chunks[2].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(
        password_underline,
        Rect {
            x: auth_chunks[2].x,
            y: auth_chunks[2].y + 1,
            width: auth_chunks[2].width,
            height: 1,
        },
    );

    // Instructions
    let instructions = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Press "),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to switch fields | "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to create account"),
        ]),
        Line::from(vec![
            Span::raw("Press "),
            Span::styled(
                "Esc",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to go back to login"),
        ]),
    ])
    .style(Style::default().fg(Color::Gray))
    .alignment(Alignment::Center);
    frame.render_widget(instructions, auth_chunks[4]);

    // Set cursor position
    match app.auth_field_focus {
        AuthField::Username => {
            frame.set_cursor_position((
                auth_chunks[0].x + 10 + app.auth_username_input.len() as u16,
                auth_chunks[0].y,
            ));
        }
        AuthField::Password => {
            frame.set_cursor_position((
                auth_chunks[2].x + 10 + app.auth_password_input.len() as u16,
                auth_chunks[2].y,
            ));
        }
    }
}

fn draw_room_list_screen(frame: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(format!(
        " Welcome, {}! ",
        app.username.as_ref().unwrap_or(&"User".to_string())
    ))
    .style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick),
    );
    frame.render_widget(header, chunks[0]);

    // Room list
    let room_items: Vec<ListItem> = app
        .rooms
        .iter()
        .enumerate()
        .map(|(i, room)| {
            let content = format!(
                "{:>2}. {} ({} users)",
                i + 1,
                room.room_name,
                room.user_count
            );
            ListItem::new(content).style(Style::default().fg(Color::White))
        })
        .collect();

    let rooms_list = List::new(room_items)
        .block(
            Block::default()
                .title(" Available Rooms ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(rooms_list, chunks[1]);

    // Instructions
    let instructions = Paragraph::new(vec![Line::from(
        "Press 1-9 to join a room | Press R to refresh | Press Q to quit",
    )])
    .style(Style::default().fg(Color::DarkGray))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(instructions, chunks[2]);
}

fn draw_chat_screen(frame: &mut Frame, app: &mut AppState, room_name: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(6),
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(format!(
        " {} - {} ",
        room_name,
        app.username.as_ref().unwrap_or(&"User".to_string())
    ))
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick),
    );
    frame.render_widget(header, chunks[0]);

    // Split the main area into messages and avatar sidebar
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(20)])
        .split(chunks[1]);

    // Messages area
    let messages_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let messages_area = messages_block.inner(main_chunks[0]);
    frame.render_widget(messages_block, main_chunks[0]);

    // Avatar sidebar
    let sidebar_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Users ");
    frame.render_widget(sidebar_block.clone(), main_chunks[1]);
    let sidebar_area = sidebar_block.inner(main_chunks[1]);

    // Tick the avatar animation
    app.avatar_manager.tick();

    // Build a list of users from room_users and always include System
    let mut user_list: Vec<String> = vec!["System".to_string()];

    // Add users from room_users for current room
    if let Some((room_id, _)) = &app.current_room
        && let Some(users) = app.room_users.get(room_id)
    {
        for user in users {
            if !user_list.contains(user) {
                user_list.push(user.clone());
            }
        }
    }

    // Sort users (System first, then alphabetical)
    user_list[1..].sort();

    // Draw avatars in sidebar
    let avatar_height = 3; // Each avatar is 3 lines tall
    let avatar_width = 8; // Width of avatar part
    let spacing = 1;
    let max_avatars = (sidebar_area.height as usize) / (avatar_height + spacing);

    // Check which users are typing in current room
    let typing_users_set = if let Some((room_id, _)) = &app.current_room {
        app.typing_users.get(room_id).cloned().unwrap_or_default()
    } else {
        std::collections::HashSet::new()
    };

    for (idx, username) in user_list.iter().take(max_avatars).enumerate() {
        // Use typing avatar if user is typing (but not for current user)
        let is_typing =
            typing_users_set.contains(username) && app.username.as_ref() != Some(username);
        let avatar_lines = if is_typing {
            app.avatar_manager.get_typing_avatar_for_user(username)
        } else {
            app.avatar_manager.get_avatar_for_user(username)
        };
        let y_offset = idx * (avatar_height + spacing);

        // Draw each line of the avatar with username to the right
        for (line_idx, avatar_line) in avatar_lines.iter().enumerate() {
            if y_offset + line_idx < sidebar_area.height as usize {
                // Draw avatar - use different width for typing avatars with bubble
                let line_width = if is_typing && line_idx == 0 {
                    avatar_width + 7 // Extra space for speech bubble
                } else {
                    avatar_width
                };

                let avatar_widget =
                    Paragraph::new(avatar_line.as_str()).style(Style::default().fg(Color::Magenta));
                frame.render_widget(
                    avatar_widget,
                    Rect {
                        x: sidebar_area.x,
                        y: sidebar_area.y + (y_offset + line_idx) as u16,
                        width: line_width.min(sidebar_area.width),
                        height: 1,
                    },
                );

                // Draw username on the middle line of the avatar
                if line_idx == 1 && sidebar_area.width > avatar_width + 1 {
                    let username_color = if username == "System" {
                        Color::Yellow
                    } else {
                        Color::Green
                    };
                    let username_widget = Paragraph::new(username.as_str()).style(
                        Style::default()
                            .fg(username_color)
                            .add_modifier(Modifier::BOLD),
                    );
                    frame.render_widget(
                        username_widget,
                        Rect {
                            x: sidebar_area.x + avatar_width + 1,
                            y: sidebar_area.y + (y_offset + line_idx) as u16,
                            width: sidebar_area.width.saturating_sub(avatar_width + 1),
                            height: 1,
                        },
                    );
                }
            }
        }
    }

    // Render messages (without inline avatars now)
    let messages: Vec<Line> = app
        .messages
        .iter()
        .filter(|msg| {
            if let Some((room_id, _)) = &app.current_room {
                &msg.room_id == room_id
            } else {
                false
            }
        })
        .flat_map(|msg| {
            vec![Line::from(vec![
                Span::styled(
                    format!("[{}] ", msg.timestamp.format("%H:%M:%S")),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{}: ", msg.username),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(&msg.text),
            ])]
        })
        .collect();

    let messages_widget = Paragraph::new(messages).wrap(Wrap { trim: true }).scroll((
        app.messages
            .len()
            .saturating_sub(messages_area.height as usize) as u16,
        0,
    ));
    frame.render_widget(messages_widget, messages_area);

    // Input area with typing indicator and help text
    let input_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(chunks[2]);

    // Typing indicator
    if let Some((room_id, _)) = &app.current_room
        && let Some(typing_users) = app.typing_users.get(room_id)
    {
        let typing_users: Vec<&String> = typing_users
            .iter()
            .filter(|u| app.username.as_ref() != Some(u))
            .collect();

        if !typing_users.is_empty() {
            let typing_text = if typing_users.len() == 1 {
                format!("{} is typing...", typing_users[0])
            } else if typing_users.len() == 2 {
                format!("{} and {} are typing...", typing_users[0], typing_users[1])
            } else {
                format!(
                    "{} and {} others are typing...",
                    typing_users[0],
                    typing_users.len() - 1
                )
            };

            // Animated dots based on current time
            let dots = match std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                / 500
                % 4
            {
                0 => "",
                1 => ".",
                2 => "..",
                _ => "...",
            };

            let typing_indicator =
                Paragraph::new(format!("{}{}", typing_text.trim_end_matches('.'), dots)).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::ITALIC),
                );
            frame.render_widget(typing_indicator, input_chunks[0]);
        }
    }

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" Type your message ");

    let input = Paragraph::new(app.input_buffer.as_str())
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .block(input_block);
    frame.render_widget(input, input_chunks[1]);

    // Help text
    let help_text = Paragraph::new("Press Esc to leave room | /quit to exit")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(help_text, input_chunks[2]);

    // Show cursor
    frame.set_cursor_position((
        input_chunks[1].x + 1 + app.input_buffer.len() as u16,
        input_chunks[1].y + 1,
    ));
}

fn draw_error_popup(frame: &mut Frame, error: &str) {
    let area = centered_rect(60, 20, frame.area());

    let popup_block = Block::default()
        .title(" Error ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .style(Style::default().fg(Color::Red));

    let error_text = Paragraph::new(error)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(Color::White))
        .block(popup_block);

    frame.render_widget(Clear, area);
    frame.render_widget(error_text, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
