// examples/tui.rs

use std::time::Duration;

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyModifiers},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use shenron::{Result, Server, Session, tui};

/// Messages pushed into the loop from background tasks.
enum Msg {
    Tick,
}

struct State {
    count: i32,
    ticks: u32,
    message: String,
}

async fn counter(session: &mut Session) -> Result {
    let mut tui = session.tui::<Msg>()?.alt_screen();

    // Server-push: a ticker wakes the loop once a second.
    let tx = tui.sender();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tx.send(Msg::Tick).is_err() {
                break;
            }
        }
    });

    let mut state = State {
        count: 0,
        ticks: 0,
        message: String::new(),
    };

    loop {
        tui.draw(|frame| draw_ui(frame, &state)).await?;

        match tui.next().await {
            Some(tui::Event::Key(key)) => match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Up | KeyCode::Char('k') => {
                    state.count += 1;
                    state.message = format!("Incremented to {}", state.count);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.count = state.count.saturating_sub(1);
                    state.message = format!("Decremented to {}", state.count);
                }
                KeyCode::Char('r') => {
                    state.count = 0;
                    state.message = "Reset!".to_string();
                }
                KeyCode::Char(c) => state.message = format!("You pressed: {c}"),
                _ => {}
            },
            Some(tui::Event::App(Msg::Tick)) => state.ticks += 1,
            Some(tui::Event::Resize(_)) => {}
            Some(tui::Event::Eof) | None => break,
        }
    }

    tui.close().await?;
    session.exit(0)
}

fn draw_ui(frame: &mut Frame, state: &State) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(frame.area());

    let title = Paragraph::new(format!("🦀 Shenron TUI Demo — up {}s", state.ticks))
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    let counter = Paragraph::new(format!("Count: {}", state.count))
        .style(Style::default().fg(if state.count >= 0 {
            Color::Green
        } else {
            Color::Red
        }))
        .block(Block::default().borders(Borders::ALL).title("Counter"));
    frame.render_widget(counter, chunks[1]);

    let help = Paragraph::new(format!(
        "{}\n\n↑/k: increment  ↓/j: decrement  r: reset  q: quit",
        state.message
    ))
    .style(Style::default().fg(Color::Gray))
    .block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(help, chunks[2]);
}

#[tokio::main]
async fn main() -> Result {
    println!("Starting TUI server on 127.0.0.1:2222");
    println!("Connect with: ssh localhost -p 2222");

    Server::new()
        .bind("0.0.0.0:2222")
        .app(counter)
        .serve()
        .await
}
