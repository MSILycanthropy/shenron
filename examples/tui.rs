// examples/tui.rs

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use shenron::{
    Server,
    tui::{App, Ratatui},
};

#[derive(Clone)]
struct Counter {
    count: i32,
    message: String,
}

impl Counter {
    const fn new() -> Self {
        Self {
            count: 0,
            message: String::new(),
        }
    }
}

impl App for Counter {
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
            KeyCode::Up | KeyCode::Char('k') => {
                self.count += 1;
                self.message = format!("Incremented to {}", self.count);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.count = self.count.saturating_sub(1);
                self.message = format!("Decremented to {}", self.count);
            }
            KeyCode::Char('r') => {
                self.count = 0;
                self.message = "Reset!".to_string();
            }
            KeyCode::Char(c) => {
                self.message = format!("You pressed: {c}");
            }
            _ => {}
        }
        true
    }

    fn draw(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(frame.area());

        // Title
        let title = Paragraph::new("🦀 Shenron TUI Demo")
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, chunks[0]);

        // Counter
        let counter = Paragraph::new(format!("Count: {}", self.count))
            .style(Style::default().fg(if self.count >= 0 {
                Color::Green
            } else {
                Color::Red
            }))
            .block(Block::default().borders(Borders::ALL).title("Counter"));
        frame.render_widget(counter, chunks[1]);

        // Help / Message
        let help = Paragraph::new(format!(
            "{}\n\n↑/k: increment  ↓/j: decrement  r: reset  q: quit",
            self.message
        ))
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL).title("Help"));
        frame.render_widget(help, chunks[2]);
    }
}

#[tokio::main]
async fn main() -> shenron::Result<()> {
    println!("Starting TUI server on 127.0.0.1:2222");
    println!("Connect with: ssh localhost -p 2222");

    let key =
        russh::keys::PrivateKey::random(&mut rand::rngs::OsRng, russh::keys::Algorithm::Ed25519)
            .expect("Failed to create key");

    Server::new()
        .bind("0.0.0.0:2222")
        .host_key(key)
        .app(Ratatui {
            app: Counter::new(),
        })
        .serve()
        .await
}
