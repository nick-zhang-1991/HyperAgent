//! HyperAgent TUI — terminal dashboard
//!
//! Provides a real-time status view of the agent's state:
//! index stats, memory usage, session list, and current mode.
//!
//! Usage: `hyper tui`

use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use std::io;
use std::time::Duration;

/// Run the TUI dashboard
pub async fn run_tui() -> Result<()> {
    // Setup terminal
    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    let backend = CrosstermBackend::new(&mut stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Main loop
    let res = run_app(&mut terminal).await;

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        io::stdout(),
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    )?;

    res
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<&mut io::Stdout>>) -> Result<()> {
    let tick_rate = Duration::from_millis(250);

    loop {
        terminal.draw(|f| {
            let size = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),   // Title
                    Constraint::Min(5),      // Main content
                    Constraint::Length(3),   // Footer
                ])
                .split(size);

            // Title
            let title = Paragraph::new(Text::from(
                Line::from(vec![
                    Span::styled(" HyperAgent ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw("v0.1.0"),
                ])
            ))
            .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Cyan)));
            f.render_widget(title, chunks[0]);

            // Main content — placeholder info
            let info = Paragraph::new(Text::from(
                "Welcome to HyperAgent TUI!\n\n\
                 Features:\n\
                 • Code indexing with PageRank\n\
                 • Multi-agent parallel execution\n\
                 • Intelligent code generation\n\
                 • Memory persistence\n\n\
                 Press 'q' to quit, 'h' for help."
            ))
            .block(Block::default().borders(Borders::ALL).title("Dashboard"))
            .wrap(Wrap { trim: false });
            f.render_widget(info, chunks[1]);

            // Footer
            let mode = Paragraph::new(Text::from(
                Line::from(vec![
                    Span::raw(" [Q]uit  "),
                    Span::styled("[H]elp", Style::default().fg(Color::Green)),
                ])
            ))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(mode, chunks[2]);
        })?;

        // Check for key press
        if crossterm::event::poll(tick_rate)? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                match key.code {
                    crossterm::event::KeyCode::Char('q') => break,
                    crossterm::event::KeyCode::Char('Q') => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
