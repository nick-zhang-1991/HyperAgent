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
    }

    fn submit_input(&mut self) -> String {
        let trimmed = self.input.trim().to_string();
        if !trimmed.is_empty() {
            self.input_history.push(trimmed.clone());
        }
        self.input.clear();

        self.input_history_idx = None;
        trimmed
    }

    fn insert_char(&mut self, ch: char) {
        self.input.push(ch);
    }

    fn insert_str(&mut self, s: &str) {
        self.input.push_str(s);
    }

    fn backspace(&mut self) {
        self.input.pop();
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
    let model_short = if model.len() > 25 {
        format!("{}…", &model[..23])
    } else {
        model.clone()
    };

    let text = Line::from(vec![
        Span::styled(" HyperAgent ", Style::default().fg(Color::Cyan)),
        Span::raw("· "),
        Span::styled(&app.mode, Style::default().fg(Color::Green)),
        Span::raw(" · "),
        Span::styled(&model_short, Style::default().fg(Color::Yellow)),
    ]);

    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, area);
}

fn render_history(frame: &mut Frame, area: Rect, app: &mut AppState) {
    // Build the text from history
    let mut lines: Vec<Line> = Vec::new();

    // Add a small hint if empty
    if app.history.is_empty() && !app.is_processing {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  欢迎使用 HyperAgent", Style::default().fg(Color::Blue).bold()),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(
            Span::styled(
                "  可以做什么？直接输入问题，或使用以下命令：",
                Style::default().fg(Color::DarkGray)
            )
        ));
        lines.push(Line::from(
            Span::styled(
                "    /mode <general|ask|code|debug|architect>  切换模式",
                Style::default().fg(Color::Gray)
            )
        ));
        lines.push(Line::from(
            Span::styled(
                "    /code <prompt>  强制走编码管道（带项目索引）",
                Style::default().fg(Color::Gray)
            )
        ));
        lines.push(Line::from(
            Span::styled(
                "    /help           查看所有命令",
                Style::default().fg(Color::Gray)
            )
        ));
        lines.push(Line::from(""));
        lines.push(Line::from(
            Span::styled(
                "  Shift+Enter 换行 · PgUp/PgDn 滚动 · ↑↓ 历史 · Ctrl+C 退出",
                Style::default().fg(Color::Gray)
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
    let prefix = "❯ ";

    // Build the input display text — placeholder when empty
    let display_text = if app.input.is_empty() && !app.is_processing {
        Text::from(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Blue)),
            Span::styled(
                "可以做什么？输入 /help 查看命令",
                Style::default().fg(Color::DarkGray).italic()
            ),
        ]))
    } else if app.input.is_empty() && app.is_processing {
        Text::from(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::DarkGray)),
            Span::styled("请稍候...", Style::default().fg(Color::Gray).italic()),
        ]))
    } else {
        let all_text = app.input.clone();
        let text = Text::from(Line::from(
            vec![
                Span::styled(prefix, Style::default().fg(Color::Blue)),
                Span::raw(all_text),
            ]
        ));
        return render_input_box(frame, area, app, text);
    };

    render_input_box(frame, area, app, display_text);
}

fn render_input_box(frame: &mut Frame, area: Rect, app: &AppState, text: Text) {
    let border_style = if app.is_processing {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(Color::Blue)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    // Place cursor AFTER the text (at the end of input)
    if !app.input.is_empty() && !app.is_processing {
        let text_x = (2 + (app.input.chars().count() as u16).min(inner.width.saturating_sub(3)))
            .min(inner.width.saturating_sub(1));
        frame.set_cursor(inner.x + text_x, inner.y);
    }
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

        // Input history
        KeyCode::Up => app.navigate_input_history(1),
        KeyCode::Down => app.navigate_input_history(-1),

        // History scroll
        KeyCode::PageUp => app.scroll_history(10),
        KeyCode::PageDown => app.scroll_history(-10),

        // Clear input
        KeyCode::Char('u') | KeyCode::Char('U') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.clear();
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
                            } else {
                                app.input_history.push(prompt.clone());
                                app.input.clear();
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
