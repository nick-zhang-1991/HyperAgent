//! HyperAgent TUI REPL — Hermes-style interactive mode
//!
//! Uses crossterm directly (not ratatui) for a non-full-screen layout:
//!   * Conversation output scrolls in the terminal naturally.
//!   * Input area stays fixed at the bottom, framed by divider lines.
//!   * Variable-height input: grows as user types, capped at 8 visual lines.
//!
//! Keybindings:
//!   Enter           Submit prompt
//!   Shift+Enter     Newline in input
//!   ↑↓              Input history
//!   Ctrl+C / Ctrl+D Exit
//!   Ctrl+U          Clear input
//!   Esc             Clear input

#![cfg(feature = "tui")]

use anyhow::Result;
use crossterm::cursor::{self, MoveTo};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::style::{self, Color, PrintStyledContent, Stylize};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::QueueableCommand;
use std::io::{self, Write};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::llm::{LlmProvider, Message};
use crate::repl::get_provider_from_config;

/// Max visual lines the input area can occupy (includes the two divider lines).
const MAX_INPUT_AREA_LINES: u16 = 12;
/// Tick rate for processing LLM responses
const TICK_MS: u64 = 50;

// ── Cursor/display helpers ──────────────────────────────────────────

fn size() -> Result<(u16, u16)> {
    let (w, h) = terminal::size()?;
    Ok((w, h))
}

fn clear_line(stdout: &mut io::Stdout, row: u16) -> Result<()> {
    stdout.queue(MoveTo(0, row))?;
    stdout.queue(Clear(ClearType::CurrentLine))?;
    Ok(())
}

fn write_at(stdout: &mut io::Stdout, row: u16, col: u16, text: &str) -> Result<()> {
    stdout.queue(MoveTo(col, row))?;
    write!(stdout, "{text}")?;
    Ok(())
}

/// Compute how many visual rows a string occupies at a given column width.
/// Each logical line wraps to ceil(chars / width) visual rows.
fn visual_lines(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut total = 0usize;
    for line in text.lines() {
        let cnt = line.chars().count();
        total += if cnt == 0 { 1 } else { (cnt + width - 1) / width };
    }
    total.max(1)
}

/// Return the input-area height in terminal rows.
/// base = 2 (top divider + bottom divider) + 1 (min input line).
/// Additional rows added as the input text wraps.
fn input_area_height(input: &str, term_width: u16) -> u16 {
    let inner = (term_width.saturating_sub(2)) as usize; // "❯ " prefix margin
    let text_rows = visual_lines(input, inner.max(1));
    (2 + text_rows).min(MAX_INPUT_AREA_LINES as usize) as u16
}

/// Redraw the input area at a given starting row.
/// Layout:
///   row+0:   ───── (top divider, bronze)
///   row+1:   ❯ {input text}
///   row+N:   (wrapped lines)
///   row+N+1: ───── (bottom divider, bronze)
fn draw_input_area(
    stdout: &mut io::Stdout,
    input: &str,
    term_width: u16,
    start_row: u16,
) -> Result<()> {
    let inner = (term_width.saturating_sub(2)) as usize;
    let divider = "─".repeat(term_width.saturating_sub(1) as usize);

    // Top divider — bronze
    stdout.queue(MoveTo(0, start_row))?;
    stdout.queue(style::SetForegroundColor(Color::Rgb {
        r: 205, g: 127, b: 50,
    }))?;
    write!(stdout, "{divider}")?;
    stdout.queue(style::ResetColor)?;

    // Input text lines
    let mut row = start_row + 1;
    let text_rows = visual_lines(input, inner.max(1));
    let max_rows = (MAX_INPUT_AREA_LINES - 2).min(text_rows as u16);

    // Build the text lines (with word wrapping)
    let mut visual_lines_vec: Vec<String> = Vec::new();
    if input.is_empty() {
        visual_lines_vec.push(String::new());
    } else {
        for line in input.lines() {
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            if chars.is_empty() {
                visual_lines_vec.push(String::new());
            }
            while pos < chars.len() {
                let end = (pos + inner).min(chars.len());
                let segment: String = chars[pos..end].iter().collect();
                visual_lines_vec.push(segment);
                pos = end;
            }
        }
    }
    if visual_lines_vec.is_empty() {
        visual_lines_vec.push(String::new());
    }

    for i in 0..max_rows {
        let line = if (i as usize) < visual_lines_vec.len() {
            &visual_lines_vec[i as usize]
        } else {
            ""
        };
        clear_line(stdout, row)?;
        stdout.queue(MoveTo(0, row))?;
        if i == 0 {
            write!(stdout, "❯ {line}")?;
        } else {
            write!(stdout, "  {line}")?;
        }
        row += 1;
    }

    // Clear any remaining lines between input text and bottom divider
    let text_end_row = start_row + 1 + max_rows;
    while text_end_row < row {
        clear_line(stdout, row)?;
        row += 1;
    }
    // Recalculate row in case we advanced past where the bottom divider should be
    let bot_row = (start_row + 1 + max_rows).max(
        (start_row + 1 + (MAX_INPUT_AREA_LINES - 2).min(text_rows as u16) + 1).min(terminal::size()?.1.saturating_sub(1))
    );

    // Bottom divider (bronze)
    let actual_bot_row = start_row + 1 + max_rows;
    if actual_bot_row < terminal::size()?.1 {
        clear_line(stdout, actual_bot_row)?;
        stdout.queue(MoveTo(0, actual_bot_row))?;
        stdout.queue(style::SetForegroundColor(Color::Rgb {
            r: 205, g: 127, b: 50,
        }))?;
        write!(stdout, "{divider}")?;
        stdout.queue(style::ResetColor)?;
    }

    // Position cursor at the end of the input text (last visual line, correct column)
    if !input.is_empty() {
        let last_visual_line = visual_lines_vec.last().map(|s| s.chars().count()).unwrap_or(0);
        let cursor_row = start_row + 1 + (max_rows.saturating_sub(1));
        let cursor_col = if last_visual_line == 0 { 2u16 } else { (2 + last_visual_line) as u16 };
        stdout.queue(MoveTo(cursor_col.min(term_width.saturating_sub(1)), cursor_row))?;
    } else {
        // Place cursor at the prompt position on the first line
        stdout.queue(MoveTo(2, start_row + 1))?;
    }

    stdout.flush()?;
    Ok(())
}

// ── State ───────────────────────────────────────────────────────────

struct AppState {
    input: String,
    history: Vec<(String, String)>,   // (user prompt, assistant response)
    input_history: Vec<String>,        // previously submitted prompts
    input_history_idx: Option<usize>,
    provider: LlmProvider,
    mode: String,
    is_processing: bool,
    should_exit: bool,
    /// Total number of terminal lines the conversation output has consumed.
    output_lines: usize,
}

impl AppState {
    fn new(provider: LlmProvider) -> Self {
        Self {
            input: String::new(),
            history: Vec::new(),
            input_history: Vec::new(),
            input_history_idx: None,
            provider,
            mode: "general".into(),
            is_processing: false,
            should_exit: false,
            output_lines: 0,
        }
    }

    fn submit_prompt(&mut self) -> String {
        let trimmed = self.input.trim().to_string();
        if !trimmed.is_empty() {
            self.input_history.push(trimmed.clone());
        }
        self.input.clear();
        self.input_history_idx = None;
        trimmed
    }

    fn navigate_input_history(&mut self, direction: i32) {
        if self.input_history.is_empty() {
            return;
        }
        match self.input_history_idx {
            Some(i) => {
                let total = self.input_history.len();
                if direction > 0 && i > 0 {
                    let s = self.input_history[i - 1].clone();
                    self.input = s;
                    self.input_history_idx = Some(i - 1);
                } else if direction < 0 {
                    if i + 1 < total {
                        let s = self.input_history[i + 1].clone();
                        self.input = s;
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
                    let s = self.input_history[last].clone();
                    self.input = s;
                    self.input_history_idx = Some(last);
                }
            }
        }
    }
}

// ── LLM call ───────────────────────────────────────────────────────

fn make_system_prompt(mode: &str) -> String {
    match mode {
        "ask" => "You are HyperAgent — a universal AI assistant. Answer concisely.\n\
                  Format code with ```language```.\n\
                  Answer in the same language as the question.".into(),
        "general" => "You are HyperAgent — a versatile AI agent capable of ANY task.\n\
                      You handle: coding, research, writing, data analysis, and more.\n\
                      Format code with ```language```.\n\
                      Answer in the same language as the question.".into(),
        "code" => "You are HyperAgent in code mode. Answer coding questions.\n\
                   Format code with ```language```.".into(),
        "debug" => "You are HyperAgent in debug mode. Root-cause focused.\n\
                    Format code with ```language```.".into(),
        "architect" => "You are HyperAgent in architect mode. Design-focused.\n\
                        Discuss trade-offs. Answer in the user's language.".into(),
        _ => "You are HyperAgent — a universal AI agent. Answer concisely.".into(),
    }
}

async fn call_llm(
    prompt: &str,
    history: &[(String, String)],
    provider: &LlmProvider,
    mode: &str,
) -> String {
    let system = make_system_prompt(mode);
    let mut messages = vec![Message::text("system", system)];
    for (u, a) in history {
        messages.push(Message::text("user", u.clone()));
        messages.push(Message::text("assistant", a.clone()));
    }
    messages.push(Message::text("user", prompt.to_string()));
    match provider.chat(messages).await {
        Ok(r) => r,
        Err(e) => format!("⚠️  Error: {e}"),
    }
}

// ── Main REPL ──────────────────────────────────────────────────────

pub async fn run_repl_tui() -> Result<()> {
    let provider = get_provider_from_config();
    let mut app = AppState::new(provider);

    let mut stdout = io::stdout();

    // Setup raw mode — no alternate screen!
    terminal::enable_raw_mode()?;
    stdout.queue(cursor::Hide)?;
    stdout.flush()?;

    // Print welcome message
    println!();
    println!("  \x1b[34mHyperAgent\x1b[0m — \x1b[32m{}\x1b[0m · \x1b[33m{}\x1b[0m",
        app.mode, app.provider.model);
    println!("  \x1b[90m可以直接输入问题，或使用 /help 查看命令。Shift+Enter 换行，Ctrl+C 退出。\x1b[0m");
    println!();

    // Record that we printed 4 lines
    app.output_lines = 4;

    // Channel for LLM responses
    let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<String>();

    // Event loop
    loop {
        let (columns, rows) = terminal::size()?;
        let ih = input_area_height(&app.input, columns);
        let prompt_start_row = rows.saturating_sub(ih);

        // Draw input area at the bottom
        draw_input_area(&mut stdout, &app.input, columns, prompt_start_row)?;

        // Check for LLM response
        if app.is_processing {
            if let Ok(response) = llm_rx.try_recv() {
                app.is_processing = false;
                // Find the last entry in history (added when prompt was submitted)
                if let Some((prompt, _)) = app.history.last() {
                    let p = prompt.clone();
                    // Update the last history entry with the actual response
                    let len = app.history.len();
                    app.history[len - 1].1 = response;
                }
                // Redraw: print the conversation output above the input area
                app.output_lines += 2; // user + assistant lines (approximate)
                // Actually, we need to re-output the conversation
                redraw_output(&mut stdout, &app.history,
                    prompt_start_row.saturating_sub(1) as usize)?;
            }
        }

        // Read key event with a short timeout
        if event::poll(Duration::from_millis(TICK_MS))? {
            if let Event::Key(key) = event::read()? {
                // Global specials
                match (key.code, key.modifiers) {
                    // Ctrl+C or Ctrl+D → exit
                    (KeyCode::Char('c'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('C'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('d'), KeyModifiers::CONTROL)
                    | (KeyCode::Char('D'), KeyModifiers::CONTROL) => break,
                    _ => {}
                }

                if app.should_exit {
                    break;
                }

                // Normal key handling
                match key.code {
                    KeyCode::Enter => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            app.input.push('\n');
                        } else {
                            let prompt = app.submit_prompt();
                            if !prompt.is_empty() {
                                if prompt.starts_with('/') {
                                    // Handle commands
                                    let cmd = prompt.trim();
                                    match cmd {
                                        "/exit" | "/quit" | "/q" => break,
                                        "/help" | "/h" => {
                                            app.history.push(("Help".into(),
                                                "HyperAgent — Universal AI Agent\n\n\
                                                 /exit, /quit       Exit\n\
                                                 /mode <mode>       Switch mode (general/ask/code/debug/architect)\n\
                                                 /clear             Clear history\n\
                                                 /code <prompt>     Force code path\n\
                                                 /help              Show this help\n\n\
                                                 Shift+Enter        Newline\n\
                                                 Ctrl+C / Ctrl+D   Exit\n\
                                                 Ctrl+U            Clear input\n\
                                                 ↑↓                Input history".into()));
                                        }
                                        "/clear" | "/cls" => {
                                            app.history.clear();
                                        }
                                        "/mode" => {
                                            app.history.push(("Info".into(),
                                                format!("Current mode: {}", app.mode)));
                                        }
                                        c if c.starts_with("/mode ") => {
                                            let m = c[6..].trim();
                                            let valid = ["general", "ask", "code", "debug", "architect"];
                                            if valid.contains(&m) {
                                                app.mode = m.to_string();
                                                app.history.push(("Info".into(),
                                                    format!("Mode: {m}")));
                                            } else {
                                                app.history.push(("Info".into(),
                                                    format!("Unknown mode: {m}. Options: {}", valid.join(", "))));
                                            }
                                        }
                                        c if c.starts_with("/code ") => {
                                            let code_prompt = c[6..].trim().to_string();
                                            // Submit as coding task via LLM
                                            app.history.push((code_prompt.clone(), String::new()));
                                            let idx = app.history.len() - 1;
                                            app.is_processing = true;
                                            let p = app.provider.clone();
                                            let mode = app.mode.clone();
                                            let tx = llm_tx.clone();
                                            tokio::spawn(async move {
                                                let resp = call_llm(&code_prompt, &[], &p, &mode).await;
                                                tx.send(resp).ok();
                                            });
                                        }
                                        _ => {
                                            app.history.push(("Info".into(),
                                                format!("Unknown: {cmd}. /help")));
                                        }
                                    }
                                    app.output_lines += 2;
                                } else {
                                    // Normal prompt — submit to LLM
                                    app.history.push((prompt.clone(), String::new()));
                                    app.is_processing = true;
                                    let p = app.provider.clone();
                                    let h = app.history[..app.history.len() - 1].to_vec();
                                    let mode = app.mode.clone();
                                    let tx = llm_tx.clone();
                                    tokio::spawn(async move {
                                        let resp = call_llm(&prompt, &h, &p, &mode).await;
                                        tx.send(resp).ok();
                                    });
                                }
                            }
                        }
                    }
                    KeyCode::Char(ch) => app.input.push(ch),
                    KeyCode::Backspace => { app.input.pop(); }
                    KeyCode::Up => app.navigate_input_history(1),
                    KeyCode::Down => app.navigate_input_history(-1),
                    KeyCode::Esc => { app.input.clear(); }
                    KeyCode::Char('u') | KeyCode::Char('U')
                        if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.input.clear();
                    }
                    _ => {}
                }
            }
        }

        // Resize detection: just loop redraw
    }

    // Cleanup
    stdout.queue(cursor::Show)?;
    stdout.flush()?;
    terminal::disable_raw_mode()?;

    Ok(())
}

/// Reprint the conversation output above the input area.
/// This is simplistic — for a polished version we'd manage scroll regions.
fn redraw_output(
    stdout: &mut io::Stdout,
    history: &[(String, String)],
    _max_row: usize,
) -> Result<()> {
    // This function is a placeholder; in our inline-terminal approach,
    // conversation output is naturally printed via println! so history
    // is just the terminal scrollback.
    // For now, we don't need to redraw if the content stays above.
    Ok(())
}
