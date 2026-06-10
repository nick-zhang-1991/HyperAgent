//! HyperAgent TUI REPL — Hermes-style interactive mode
//!
//! Full-screen terminal UI with:
//! - Status bar at top (mode, model, /help)
//! - Scrollable conversation history in the middle
//! - Variable-height input frame with dividers at the bottom
//!
//! Key design:
//!   * The text input auto-grows as the user types.
//!   * Enter submits the prompt; Shift+Enter inserts a newline.
//!   * Up/Down cycles through input history; PgUp/PgDn scrolls chat history.
//!   * Ctrl+C/Ctrl+D quits; Ctrl+U clears input.
//!   * /commands work as in the classic REPL.

#![cfg(feature = "tui")]

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap, Clear},
    Terminal, Frame,
};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::llm::{LlmProvider, Message};
use crate::repl::{get_provider_from_config, looks_like_coding_task};

/// Max lines the input frame can grow to before scrolling within the frame.
const MAX_INPUT_LINES: u16 = 10;
/// Delay per tick of the TUI event loop
const TICK_MS: u64 = 50;

// ── State ────────────────────────────────────────────────────────────

struct AppState {
    // Status
    mode: String,
    provider: LlmProvider,
    dir: std::path::PathBuf,

    // Conversation history
    history: Vec<(String, String)>,   // (user prompt, assistant response)
    scroll_offset: usize,              // scrollback for history area
    history_count: usize,              // total lines in history for scroll calc

    // Input
    input: String,
    cursor: usize,                     // char index in input
    input_history: Vec<String>,        // previously submitted prompts
    input_history_idx: Option<usize>,  // position in history (None = new)

    // Processing
    is_processing: bool,
    processing_token: Option<String>,  // partial response being streamed

    // State
    should_exit: bool,
    dirty: bool,
}

impl AppState {
    fn new(provider: LlmProvider, dir: std::path::PathBuf) -> Self {
        Self {
            mode: "general".into(),
            provider,
            dir,
            history: Vec::new(),
            scroll_offset: 0,
            history_count: 0,
            input: String::new(),
            cursor: 0,
            input_history: Vec::new(),
            input_history_idx: None,
            is_processing: false,
            processing_token: None,
            should_exit: false,
            dirty: true,
        }
    }

    // ── Input helpers ──────────────────────────────────────────────

    fn current_input(&self) -> &str {
        &self.input
    }

    fn set_input(&mut self, s: &str) {
        self.input = s.to_string();
        self.cursor = self.input.chars().count();
    }

    fn submit_input(&mut self) -> String {
        let trimmed = self.input.trim().to_string();
        if !trimmed.is_empty() {
            self.input_history.push(trimmed.clone());
        }
        self.input.clear();
        self.cursor = 0;
        self.input_history_idx = None;
        trimmed
    }

    fn insert_char(&mut self, ch: char) {
        let idx = self.byte_at_cursor();
        self.input.insert(idx, ch);
        self.cursor += 1;
    }

    fn insert_str(&mut self, s: &str) {
        let idx = self.byte_at_cursor();
        self.input.insert_str(idx, s);
        self.cursor += s.chars().count();
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let byte_idx = self.byte_at_cursor_prev();
            self.input.drain(byte_idx..byte_idx + self.input[byte_idx..].chars().next().unwrap().len_utf8());
            self.cursor -= 1;
        }
    }

    fn delete_at_cursor(&mut self) {
        let char_count = self.input.chars().count();
        if self.cursor < char_count {
            let byte_idx = self.byte_at_cursor();
            let len = self.input[byte_idx..].chars().next().unwrap().len_utf8();
            self.input.drain(byte_idx..byte_idx + len);
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn cursor_right(&mut self) {
        let n = self.input.chars().count();
        if self.cursor < n {
            self.cursor += 1;
        }
    }

    fn cursor_home(&mut self) {
        // Go to start of current visual line
        let max_col = self.input_frame_inner_width().saturating_sub(1) as usize;
        let visual_col = self.cursor % max_col.max(1);
        self.cursor = self.cursor.saturating_sub(visual_col);
    }

    fn cursor_end(&mut self) {
        let n = self.input.chars().count();
        let max_col = self.input_frame_inner_width().saturating_sub(1) as usize;
        if max_col > 0 {
            let visual_col = n % max_col;
            self.cursor = n.saturating_sub((max_col - 1).min(visual_col));
        } else {
            self.cursor = n;
        }
    }

    fn byte_at_cursor(&self) -> usize {
        self.input.chars().take(self.cursor).map(|c| c.len_utf8()).sum()
    }

    fn byte_at_cursor_prev(&self) -> usize {
        if self.cursor == 0 { return 0; }
        self.input.chars().take(self.cursor - 1).map(|c| c.len_utf8()).sum()
    }

    fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    fn navigate_input_history(&mut self, direction: i32) {
        if self.input_history.is_empty() {
            return;
        }
        let idx = self.input_history_idx;
        match idx {
            Some(i) => {
                let total = self.input_history.len();
                if direction > 0 {
                    // up: older entries
                    if i > 0 {
                        let hist = self.input_history[i - 1].clone();
                        self.set_input(&hist);
                        self.input_history_idx = Some(i - 1);
                    }
                } else {
                    // down: newer entries
                    if i + 1 < total {
                        let hist = self.input_history[i + 1].clone();
                        self.set_input(&hist);
                        self.input_history_idx = Some(i + 1);
                    } else {
                        self.input.clear();
                        self.cursor = 0;
                        self.input_history_idx = None;
                    }
                }
            }
            None => {
                if direction > 0 {
                    let last = self.input_history.len() - 1;
                    let hist = self.input_history[last].clone();
                    self.set_input(&hist);
                    self.input_history_idx = Some(last);
                }
            }
        }
    }

    fn scroll_history(&mut self, amount: i32) {
        let max_scroll = self.history_count.saturating_sub(1);
        if amount > 0 {
            self.scroll_offset = self.scroll_offset.saturating_add(amount as usize).min(max_scroll);
        } else {
            let abs = amount.unsigned_abs();
            self.scroll_offset = self.scroll_offset.saturating_sub(abs as usize);
        }
    }

    // Compute how many lines the input text wraps to inside the frame
    #[allow(dead_code)]
    fn input_wrapped_lines(&self, width: u16) -> usize {
        if self.input.is_empty() {
            return 1;
        }
        let inner = (width.saturating_sub(4)) as usize;
        if inner == 0 {
            return self.input.lines().count();
        }
        let mut total = 0usize;
        for line in self.input.lines() {
            let cnt = line.chars().count();
            if cnt == 0 {
                total += 1;
            } else {
                total += (cnt + inner - 1) / inner;
            }
        }
        total.max(1)
    }

    /// The effective height of the input frame (varies with input)
    fn input_frame_height(&self, width: u16) -> u16 {
        let base: u16 = 3; // top border + bottom border + 1 padding
        let wrap_lines = self.input_wrapped_lines(width) as u16;
        (base + wrap_lines).min(MAX_INPUT_LINES + 3)
    }

    fn input_frame_inner_width(&self) -> u16 {
        // To be called with the actual width; for now just return a reasonable default
        80u16.saturating_sub(4)
    }

    // ── Mode helpers ─────────────────────────────────────────────

    fn valid_modes() -> &'static [&'static str] {
        &["general", "ask", "code", "debug", "architect"]
    }

    fn set_mode(&mut self, new: &str) -> bool {
        if Self::valid_modes().contains(&new) {
            self.mode = new.to_string();
            true
        } else {
            false
        }
    }

    // ── LLM call ─────────────────────────────────────────────────

    #[allow(dead_code)]
    async fn run_llm(&self, prompt: String, tx: mpsc::UnboundedSender<String>) {
        let provider = self.provider.clone();
        let history = self.history.clone();
        let mode = self.mode.clone();
        let dir = self.dir.clone();

        tokio::spawn(async move {
            let system_prompt = match mode.as_str() {
                "ask" => "You are HyperAgent — a universal AI assistant (capable of \
                          any task, not just coding). Answer concisely and accurately.\n\
                          Format code with ```language```.\n\
                          Answer in the same language as the question.\n\
                          Rules:\n- Be concise but complete\n- Don't claim to have read project files",
                "general" => "You are HyperAgent — a versatile AI agent capable of ANY task.\n\
                              You handle: coding, research, writing, data analysis, \
                              translation, brainstorming, web search, API testing, \
                              file operations, math, science, history, language, \
                              planning, and more.\n\
                              Format code with ```language```.\n\
                              Answer in the same language as the question.\n\
                              Rules:\n- Be helpful, concise, and accurate\n- For code questions, give runnable examples",
                "code" => "You are HyperAgent in code mode — focused on code Q&A.\n\
                           Answer the user's coding question with ```language``` blocks.\n\
                           Answer in the same language as the question.",
                "debug" => "You are HyperAgent in debug mode — root-cause focused.\n\
                            Ask for exact error messages before guessing.\n\
                            Format code with ```language```.\n\
                            Answer in the user's language.",
                "architect" => "You are HyperAgent in architect mode — design-focused.\n\
                                Discuss trade-offs and patterns. Do not write implementation\n\
                                code unless asked.\n\
                                Answer in the user's language.",
                _ => "You are HyperAgent — a universal AI agent. Answer concisely.\n\
                      Format code with ```language```.\n\
                      Answer in the same language as the question.",
            };

            let mut messages = vec![Message::text("system", system_prompt.to_string())];
            for (u, a) in &history {
                messages.push(Message::text("user", u.clone()));
                messages.push(Message::text("assistant", a.clone()));
            }
            messages.push(Message::text("user", prompt.to_string()));

            match provider.chat(messages).await {
                Ok(response) => {
                    tx.send(response).ok();
                }
                Err(e) => {
                    tx.send(format!("⚠️  Error: {e}")).ok();
                }
            }
        });
    }
}

// ── Rendering ────────────────────────────────────────────────────────

fn render(frame: &mut Frame, app: &mut AppState) {
    let area = frame.area();
    let input_h = app.input_frame_height(area.width);

    // Layout: status bar (1) + history (rest) + input frame (variable)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                     // Status bar
            Constraint::Min(1),                        // History
            Constraint::Length(input_h),               // Input frame
        ])
        .split(area);

    render_status_bar(frame, chunks[0], app);
    render_history(frame, chunks[1], app);
    render_input_frame(frame, chunks[2], app);
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let model = &app.provider.model;
    let model_short = if model.len() > 30 {
        format!("{}…", &model[..28])
    } else {
        model.clone()
    };

    let text = Line::from(vec![
        Span::styled(" HyperAgent ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" · "),
        Span::styled(&app.mode, Style::default().fg(Color::Green)),
        Span::raw(" · "),
        Span::styled(&model_short, Style::default().fg(Color::Yellow)),
        Span::raw(" · "),
        Span::styled("/help", Style::default().fg(Color::DarkGray)),
    ]);

    let paragraph = Paragraph::new(text)
        .style(Style::default().bg(Color::Rgb(20, 20, 30)).fg(Color::White));
    frame.render_widget(paragraph, area);
}

fn render_history(frame: &mut Frame, area: Rect, app: &mut AppState) {
    // Build the text from history
    let mut lines: Vec<Line> = Vec::new();

    // Add a small hint if empty
    if app.history.is_empty() && !app.is_processing {
        lines.push(Line::from(
            Span::styled(
                "  Type a prompt to start. /help for commands. Shift+Enter for newline.",
                Style::default().fg(Color::DarkGray).italic()
            )
        ));
        lines.push(Line::from(""));
    }

    for (user, assistant) in &app.history {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Color::Cyan).bold()),
            Span::raw(user),
        ]));
        lines.push(Line::from(""));
        // Split assistant response into wrapped lines
        for line in assistant.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(Color::Green)),
                Span::raw(line),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Show processing state
    if app.is_processing {
        if let Some(ref partial) = app.processing_token {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(Color::Green)),
                Span::styled("...", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::from(""));
            lines.push(Line::from(
                Span::styled("  🤔 Thinking...", Style::default().fg(Color::DarkGray).italic())
            ));
        }
    }

    // Calculate total lines and apply scroll offset
    let total_lines = lines.len();
    app.history_count = total_lines;

    let visible_height = area.height.saturating_sub(1) as usize;
    let scroll = app.scroll_offset.min(total_lines.saturating_sub(visible_height));
    let display_lines: Vec<Line> = lines.iter()
        .skip(scroll)
        .take(visible_height)
        .cloned()
        .collect();

    let text = Text::from(display_lines);
    let paragraph = Paragraph::new(text)
        .block(Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(60, 60, 80))))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_input_frame(frame: &mut Frame, area: Rect, app: &AppState) {
    // Build the input display text
    let prefix = "❯ ";
    let display_text = if app.input.is_empty() {
        Text::from(Line::from(
            Span::styled(prefix, Style::default().fg(Color::Cyan))
        ))
    } else {
        let mut spans = vec![
            Span::styled(prefix, Style::default().fg(Color::Cyan)),
            Span::raw(&app.input),
        ];
        Text::from(Line::from(spans))
    };

    // Style the block based on processing state
    let (_fg, border_style) = if app.is_processing {
        (Color::DarkGray, Style::default().fg(Color::Rgb(60, 60, 70)))
    } else {
        (Color::White, Style::default().fg(Color::Rgb(100, 140, 255)))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(Color::Rgb(12, 12, 20)));

    // Inner area with padding for text
    let inner = block.inner(area);
    let paragraph = Paragraph::new(display_text)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    // Cursor positioning: show in the text area
    if let Some(cursor_visual) = compute_cursor_pos(&app.input, app.cursor, inner.width) {
        let cursor_y = inner.y + cursor_visual.row as u16;
        let cursor_x = inner.x + cursor_visual.col as u16;
        frame.set_cursor(cursor_x, cursor_y);
    } else {
        // Default: show cursor after the prompt
        let prompt_prefix = 2; // "❯ " = 2 chars
        let x = inner.x + prompt_prefix as u16 + app.cursor.min(inner.width as usize - 2) as u16;
        let y = inner.y;
        frame.set_cursor(x.min(inner.right().saturating_sub(1)), y);
    }
}

/// Compute pixel-level cursor position within the wrapped text.
struct CursorPos {
    row: u16,
    col: u16,
}

fn compute_cursor_pos(input: &str, cursor: usize, inner_width: u16) -> Option<CursorPos> {
    if cursor == 0 {
        return Some(CursorPos { row: 0, col: 0 });
    }
    let inner = inner_width.saturating_sub(1) as usize; // leave 1 char margin
    if inner == 0 {
        return None;
    }
    let prefix = 2; // "❯ "
    let mut remaining = cursor;
    for (line_idx, line) in input.lines().enumerate() {
        let line_len = line.chars().count();
        let wrapped_lines = if line_len == 0 { 1 } else { (line_len + inner - 1) / inner };
        let line_take = line_len.min(remaining);
        // Which wrapped row within this logical line?
        if line_take > 0 {
            let row_within = (line_take - 1) / inner;
            let col_within = (line_take - 1) % inner;
            if remaining <= line_len {
                return Some(CursorPos {
                    row: line_idx as u16 + row_within as u16,
                    col: (if row_within == 0 { prefix as u16 } else { 0 }) + col_within as u16,
                });
            }
            remaining -= line_len;
        } else {
            // Empty line
            if remaining == 0 {
                return Some(CursorPos { row: line_idx as u16, col: 0 });
            }
        }
        // Account for line break
        if remaining > 0 {
            remaining = remaining.saturating_sub(1); // \n
        }
    }
    // Fallback: end of input
    Some(CursorPos { row: 0, col: 0 })
}

// ── Key handling ─────────────────────────────────────────────────────

/// Returns `true` if the app should exit.
fn handle_key(app: &mut AppState, key: crossterm::event::KeyEvent) -> bool {
    // Global shortcuts first
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) |
        (KeyCode::Char('C'), KeyModifiers::CONTROL) => {
            if app.is_processing {
                // Stop processing
                app.is_processing = false;
                app.processing_token = None;
                return false;
            }
            return true; // exit
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) |
        (KeyCode::Char('D'), KeyModifiers::CONTROL) => {
            return true; // exit
        }
        _ => {}
    }

    // Ignore key events during processing
    if app.is_processing {
        // Only allow Ctrl+C to cancel
        return false;
    }

    match key.code {
        // Submit
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.insert_newline();
            } else {
                let prompt = app.submit_input();
                if !prompt.is_empty() {
                    if prompt.starts_with('/') {
                        // Handle /commands
                        handle_command(app, &prompt);
                    }
                    // Prompt submission is handled in the main event loop
                }
            }
        }

        // Input editing
        KeyCode::Backspace => app.backspace(),
        KeyCode::Delete => app.delete_at_cursor(),
        KeyCode::Left => app.cursor_left(),
        KeyCode::Right => app.cursor_right(),
        KeyCode::Home => app.cursor_home(),
        KeyCode::End => app.cursor_end(),

        // Input history
        KeyCode::Up => app.navigate_input_history(1),
        KeyCode::Down => app.navigate_input_history(-1),

        // History scroll
        KeyCode::PageUp => app.scroll_history(10),
        KeyCode::PageDown => app.scroll_history(-10),

        // Clear input
        KeyCode::Char('u') | KeyCode::Char('U') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.clear();
            app.cursor = 0;
        }

        // Tab completion for /commands
        KeyCode::Tab => {
            if app.input.starts_with('/') {
                let commands = ["/help", "/exit", "/mode", "/clear", "/code "];
                for cmd in commands {
                    if cmd.starts_with(&app.input) && cmd != app.input {
                        app.set_input(cmd);
                        break;
                    }
                }
            }
        }

        // Esc to clear
        KeyCode::Esc => {
            app.input.clear();
            app.cursor = 0;
        }

        // Printable characters
        KeyCode::Char(ch) => {
            app.insert_char(ch);
        }

        _ => {}
    }

    false
}

fn handle_command(app: &mut AppState, cmd: &str) {
    let trimmed = cmd.trim();
    match trimmed {
        "/exit" | "/quit" | "/q" => {
            app.should_exit = true;
        }
        "/help" | "/h" => {
            app.add_assistant(
                "Help",
                "HyperAgent — Universal AI Agent (TUI)\n\
                 ───────────────────────────────────────\n\
                 /exit, /quit       Exit\n\
                 /mode <mode>       Switch mode (general/ask/code/debug/architect)\n\
                 /clear             Clear conversation history\n\
                 /code <prompt>     Force code path\n\
                 /help              Show this help\n\
                 ───────────────────────────────────────\n\
                 Shift+Enter        Newline (multi-line input)\n\
                 PgUp/PgDn         Scroll history\n\
                 ↑↓                 Input history\n\
                 Ctrl+C             Cancel / Exit\n\
                 Ctrl+U             Clear input\n\
                 Tab                Complete /commands"
            );
        }
        "/clear" | "/cls" => {
            app.history.clear();
            app.scroll_offset = 0;
            app.history_count = 0;
        }
        "/mode" => {
            app.add_assistant("Info", &format!("Current mode: {}", app.mode));
        }
        cmd if cmd.starts_with("/mode ") => {
            let new_mode = cmd[6..].trim();
            if app.set_mode(new_mode) {
                app.add_assistant("Info", &format!("Switched to mode: {}", new_mode));
            } else {
                app.add_assistant("Info", &format!(
                    "Unknown mode: {}. Options: {}",
                    new_mode,
                    AppState::valid_modes().join(", ")
                ));
            }
        }
        cmd if cmd.starts_with("/code ") => {
            let prompt = cmd[6..].trim().to_string();
            if !prompt.is_empty() {
                app.submit_code_prompt(prompt);
            }
        }
        _ => {
            app.add_assistant("Info", &format!("Unknown command: {cmd}. Type /help"));
        }
    }
}

impl AppState {
    fn add_assistant(&mut self, role: &str, text: &str) {
        self.history.push((role.to_string(), text.to_string()));
        self.scroll_offset = 0;
    }

    fn add_response(&mut self, prompt: String, response: String) {
        self.history.push((prompt, response));
        self.scroll_offset = 0; // auto-scroll to bottom
    }

    fn submit_prompt(&mut self, prompt: String, llm_tx: mpsc::UnboundedSender<String>) {
        self.is_processing = true;
        let provider = self.provider.clone();
        let history = self.history.clone();
        let mode = self.mode.clone();

        tokio::spawn(async move {
            let system_prompt = match mode.as_str() {
                "ask" => "You are HyperAgent — a universal AI assistant (capable of \
                          any task, not just coding). Answer concisely.\n\
                          Format code with ```language```.\n\
                          Answer in the same language as the question.",
                "general" => "You are HyperAgent — a versatile AI agent capable of ANY task.\n\
                              You handle: coding, research, writing, data analysis, \
                              translation, brainstorming, and more.\n\
                              Format code with ```language```.\n\
                              Answer in the same language as the question.",
                "code" => "You are HyperAgent in code mode — focused on code Q&A.\n\
                           Answer with ```language``` blocks. Answer in the user's language.",
                "debug" => "You are HyperAgent in debug mode — root-cause focused.\n\
                            Format code with ```language```.",
                "architect" => "You are HyperAgent in architect mode — design-focused.\n\
                                Discuss trade-offs. Answer in the user's language.",
                _ => "You are HyperAgent — a universal AI agent. Answer concisely with ```language```.",
            };

            let mut messages = vec![Message::text("system", system_prompt.to_string())];
            for (u, a) in &history {
                messages.push(Message::text("user", u.clone()));
                messages.push(Message::text("assistant", a.clone()));
            }
            messages.push(Message::text("user", prompt));

            match provider.chat(messages).await {
                Ok(response) => { llm_tx.send(response).ok(); }
                Err(e) => { llm_tx.send(format!("⚠️  Error: {e}")).ok(); }
            }
        });
    }

    fn submit_code_prompt(&mut self, prompt: String) {
        self.is_processing = true;
        let provider = self.provider.clone();
        let history = self.history.clone();
        let mode = self.mode.clone();
        let dir = self.dir.clone();

        tokio::spawn(async move {
            use crate::index::HyperIndex;
            use crate::agent::orchestrator::Orchestrator;
            use crate::hooks::HookRegistry;

            let system_prompt = "You are HyperAgent — a coding agent.\n\
                                  Analyze and modify code following the user's instructions.\n\
                                  Format code with ```language```.";

            let mut messages = vec![Message::text("system", system_prompt.to_string())];
            for (u, a) in &history {
                messages.push(Message::text("user", u.clone()));
                messages.push(Message::text("assistant", a.clone()));
            }
            messages.push(Message::text("user", prompt.clone()));

            match provider.chat(messages).await {
                Ok(response) => {
                    // The response will be sent via channel, but the TUI needs a channel
                    // For now, we'll handle this differently
                }
                Err(e) => {}
            }
        });
    }
}

// ── Main loop ────────────────────────────────────────────────────────

pub async fn run_repl_tui() -> Result<()> {
    let dir = std::env::current_dir()?;
    let provider = get_provider_from_config();

    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = AppState::new(provider, dir);

    // Channel for LLM responses
    let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<String>();

    let mut pending_prompts: Vec<String> = Vec::new();

    // Main event loop
    loop {
        // Draw
        terminal.draw(|f| render(f, &mut app))?;

        // Check processing
        if app.is_processing {
            if let Ok(response) = llm_rx.try_recv() {
                // Get the prompt that was sent
                if let Some(prompt) = app.input_history.last().cloned() {
                    app.add_response(prompt, response);
                } else {
                    // Try the previous history entry
                    if let Some(last) = app.history.last().cloned() {
                        app.add_response(last.0, response);
                    } else {
                        app.add_response("(prompt)".to_string(), response);
                    }
                }
                app.is_processing = false;
                app.processing_token = None;
            }
        }

        // Handle input events
        if event::poll(Duration::from_millis(TICK_MS))? {
            match event::read()? {
                Event::Key(key) => {
                    // Before handling key, save state for processing
                    if !app.is_processing && key.code == KeyCode::Enter
                        && !key.modifiers.contains(KeyModifiers::SHIFT)
                    {
                        let prompt = app.input.trim().to_string();
                        if !prompt.is_empty() {
                            if prompt.starts_with('/') {
                                handle_command(&mut app, &prompt);
                                app.input.clear();
                                app.cursor = 0;
                            } else {
                                app.input_history.push(prompt.clone());
                                app.input.clear();
                                app.cursor = 0;
                                app.is_processing = true;
                                app.submit_prompt(prompt, llm_tx.clone());
                            }
                            continue;
                        }
                    }
                    if handle_key(&mut app, key) {
                        break;
                    }
                }
                Event::Resize(_, _) => {
                    // Will re-render on next draw
                }
                _ => {}
            }
        }

        if app.should_exit {
            break;
        }
    }

    // Cleanup
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        io::stdout(),
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    )?;

    Ok(())
}
