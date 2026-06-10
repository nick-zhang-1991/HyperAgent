//! HyperAgent TUI REPL — Hermes-style interactive mode (inline, non-alternate-screen)
//!
//! Layout (conversation scrolls naturally in terminal scrollback):
//!
//!   ──────────────────────────────── ←─ bronze divider
//!   ❯ {input text line 1}            ←─ white background, black foreground
//!     {wrapped line 2}
//!     {wrapped line 3}
//!   ──────────────────────────────── ←─ bronze divider
//!
//! The input area is rendered with an explicit **white background / black foreground**
//! regardless of the host terminal's default palette, so the prompt is always legible.
//!
//! Keybindings:
//!   Enter             Submit prompt
//!   Shift+Enter       Newline in input
//!   ← / →             Move cursor one char
//!   Home / End        Jump to start / end of input
//!   Backspace         Delete char to the left of cursor
//!   Delete            Delete char under cursor
//!   ↑ / ↓             Navigate submitted-prompt history
//!   Ctrl+A            Jump to start
//!   Ctrl+E            Jump to end
//!   Ctrl+K            Delete from cursor to end
//!   Ctrl+U            Clear entire input
//!   Esc               Clear entire input
//!   Ctrl+C / Ctrl+D   Exit

#![cfg(feature = "tui")]

use anyhow::Result;
use crossterm::cursor::{self, MoveTo};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{self, Color, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::QueueableCommand;
use std::io::{self, Write};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use crate::llm::{LlmProvider, Message};
use crate::repl::get_provider_from_config;

/// Max visual rows the input area can occupy (top divider + text rows + bottom divider).
const MAX_INPUT_AREA_LINES: u16 = 12;
const TICK_MS: u64 = 50;
const PROMPT_PREFIX: &str = "❯ "; // 2 columns

// ── Palette ────────────────────────────────────────────────────────

const FG_BLACK: Color = Color::Rgb { r: 0, g: 0, b: 0 };
const BG_WHITE: Color = Color::Rgb {
    r: 245,
    g: 245,
    b: 245,
};
const DIVIDER: Color = Color::Rgb {
    r: 205,
    g: 127,
    b: 50,
}; // bronze

// ── Global LLM sender (single REPL at a time) ──────────────────────

static LLM_TX: OnceLock<mpsc::UnboundedSender<String>> = OnceLock::new();

fn llm_tx() -> &'static mpsc::UnboundedSender<String> {
    LLM_TX.get().expect("LLM_TX not initialized")
}

// ── Cursor / display helpers ───────────────────────────────────────

fn clear_line(stdout: &mut io::Stdout, row: u16) -> Result<()> {
    stdout.queue(MoveTo(0, row))?;
    stdout.queue(Clear(ClearType::CurrentLine))?;
    Ok(())
}

/// Display width of a single char (CJK = 2, ASCII = 1, zero-width = 0).
fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Display width of a `&str`.
fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Map a char index (0..=chars.count()) to a byte index.
fn char_to_byte_idx(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Wrap `text` into visual lines of `inner_width` columns (no prefix).
/// Existing '\n' forces a new visual line.
fn wrap_to_lines(text: &str, inner_width: usize) -> Vec<String> {
    if inner_width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_w = 0usize;
        for ch in paragraph.chars() {
            let w = char_width(ch);
            if current_w + w > inner_width && !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_w = 0;
            }
            current.push(ch);
            current_w += w;
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Input-area height in terminal rows for given `text` and `term_width`.
/// base = top divider (1) + bottom divider (1) + at least 1 text row.
fn input_area_height(text: &str, term_width: u16) -> u16 {
    let prefix_w = str_width(PROMPT_PREFIX);
    let inner = (term_width as usize).saturating_sub(prefix_w).max(1);
    let text_rows = wrap_to_lines(text, inner).len();
    (2 + text_rows).min(MAX_INPUT_AREA_LINES as usize) as u16
}

// ── State ──────────────────────────────────────────────────────────

struct AppState {
    input: String,
    /// Cursor position in **char index** (0..=input.chars().count()).
    cursor_char: usize,
    history: Vec<(String, String)>, // (user prompt, assistant response)
    input_history: Vec<String>,
    input_history_idx: Option<usize>,
    /// Saved current draft while browsing history (so Down restores it).
    history_saved_input: Option<String>,
    provider: LlmProvider,
    mode: String,
    is_processing: bool,
    should_exit: bool,
}

impl AppState {
    fn new(provider: LlmProvider) -> Self {
        Self {
            input: String::new(),
            cursor_char: 0,
            history: Vec::new(),
            input_history: Vec::new(),
            input_history_idx: None,
            history_saved_input: None,
            provider,
            mode: "general".into(),
            is_processing: false,
            should_exit: false,
        }
    }

    fn char_count(&self) -> usize {
        self.input.chars().count()
    }

    fn insert_char(&mut self, c: char) {
        let byte_pos = char_to_byte_idx(&self.input, self.cursor_char);
        self.input.insert(byte_pos, c);
        self.cursor_char += 1;
        self.history_saved_input = None;
        self.input_history_idx = None;
    }

    fn delete_backward(&mut self) {
        if self.cursor_char == 0 {
            return;
        }
        let start = char_to_byte_idx(&self.input, self.cursor_char - 1);
        let end = char_to_byte_idx(&self.input, self.cursor_char);
        self.input.replace_range(start..end, "");
        self.cursor_char -= 1;
        self.history_saved_input = None;
        self.input_history_idx = None;
    }

    fn delete_forward(&mut self) {
        if self.cursor_char >= self.char_count() {
            return;
        }
        let start = char_to_byte_idx(&self.input, self.cursor_char);
        let end = char_to_byte_idx(&self.input, self.cursor_char + 1);
        self.input.replace_range(start..end, "");
        self.history_saved_input = None;
        self.input_history_idx = None;
    }

    fn delete_to_end(&mut self) {
        let byte_pos = char_to_byte_idx(&self.input, self.cursor_char);
        self.input.truncate(byte_pos);
        self.history_saved_input = None;
        self.input_history_idx = None;
    }

    fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_char = 0;
        self.history_saved_input = None;
        self.input_history_idx = None;
    }

    fn move_left(&mut self) {
        if self.cursor_char > 0 {
            self.cursor_char -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.cursor_char < self.char_count() {
            self.cursor_char += 1;
        }
    }

    fn move_home(&mut self) {
        self.cursor_char = 0;
    }

    fn move_end(&mut self) {
        self.cursor_char = self.char_count();
    }

    fn submit_prompt(&mut self) -> String {
        let trimmed = self.input.trim().to_string();
        if !trimmed.is_empty() {
            self.input_history.push(trimmed.clone());
        }
        self.input.clear();
        self.cursor_char = 0;
        self.input_history_idx = None;
        self.history_saved_input = None;
        trimmed
    }

    fn navigate_input_history(&mut self, direction: i32) {
        if self.input_history.is_empty() {
            return;
        }
        match self.input_history_idx {
            Some(i) => {
                let total = self.input_history.len();
                if direction > 0 {
                    // Up: older
                    if i > 0 {
                        let s = self.input_history[i - 1].clone();
                        self.input = s;
                        self.cursor_char = self.char_count();
                        self.input_history_idx = Some(i - 1);
                    }
                } else {
                    // Down: newer
                    if i + 1 < total {
                        let s = self.input_history[i + 1].clone();
                        self.input = s;
                        self.cursor_char = self.char_count();
                        self.input_history_idx = Some(i + 1);
                    } else {
                        // Back to current draft
                        self.input = self.history_saved_input.take().unwrap_or_default();
                        self.cursor_char = self.char_count();
                        self.input_history_idx = None;
                    }
                }
            }
            None => {
                if direction > 0 {
                    if !self.input.is_empty() {
                        self.history_saved_input = Some(self.input.clone());
                    }
                    let last = self.input_history.len() - 1;
                    let s = self.input_history[last].clone();
                    self.input = s;
                    self.cursor_char = self.char_count();
                    self.input_history_idx = Some(last);
                }
            }
        }
    }
}

// ── Rendering ──────────────────────────────────────────────────────

fn paint_divider(stdout: &mut io::Stdout, row: u16, term_width: u16) -> Result<()> {
    clear_line(stdout, row)?;
    stdout.queue(MoveTo(0, row))?;
    stdout.queue(SetForegroundColor(DIVIDER))?;
    write!(stdout, "{}", "─".repeat(term_width.saturating_sub(1) as usize))?;
    stdout.queue(ResetColor)?;
    Ok(())
}

fn paint_input_row(
    stdout: &mut io::Stdout,
    row: u16,
    term_width: u16,
    prefix: &str,
    content: &str,
) -> Result<()> {
    clear_line(stdout, row)?;
    stdout.queue(MoveTo(0, row))?;
    stdout.queue(SetBackgroundColor(BG_WHITE))?;
    stdout.queue(SetForegroundColor(FG_BLACK))?;
    write!(stdout, "{prefix}")?;
    let remaining = term_width as usize - str_width(prefix);
    let mut taken = 0usize;
    let mut buf = String::new();
    for ch in content.chars() {
        let w = char_width(ch);
        if taken + w > remaining {
            break;
        }
        buf.push(ch);
        taken += w;
    }
    write!(stdout, "{buf}")?;
    let pad = remaining.saturating_sub(taken);
    if pad > 0 {
        write!(stdout, "{}", " ".repeat(pad.min(512)))?;
    }
    stdout.queue(ResetColor)?;
    Ok(())
}

fn draw_input_area(
    stdout: &mut io::Stdout,
    input: &str,
    cursor_char: usize,
    term_width: u16,
    term_height: u16,
    start_row: u16,
) -> Result<()> {
    let prefix_w = str_width(PROMPT_PREFIX);
    let inner = (term_width as usize).saturating_sub(prefix_w).max(1);

    paint_divider(stdout, start_row, term_width)?;

    let visual_lines = wrap_to_lines(input, inner);
    let max_text_rows = (MAX_INPUT_AREA_LINES - 2) as usize;
    let display_rows = visual_lines.len().min(max_text_rows);

    // Determine which visual row the cursor sits on, and the char offset on that row.
    let mut cum = 0usize;
    let mut cursor_visual_row: usize = 0;
    let mut cursor_col_in_row: usize = 0;
    for (i, line) in visual_lines.iter().enumerate() {
        let n = line.chars().count();
        if cursor_char <= cum + n {
            cursor_visual_row = i;
            cursor_col_in_row = cursor_char - cum;
            break;
        }
        cum += n;
        cursor_visual_row = i + 1;
        cursor_col_in_row = 0;
    }

    for i in 0..display_rows {
        let row = start_row + 1 + i as u16;
        let line_str = &visual_lines[i];
        let prefix = if i == 0 { PROMPT_PREFIX } else { "  " };
        paint_input_row(stdout, row, term_width, prefix, line_str)?;
    }

    // Pad remaining rows so the white box visually extends to the bottom divider.
    for i in display_rows..max_text_rows {
        let row = start_row + 1 + i as u16;
        clear_line(stdout, row)?;
        stdout.queue(MoveTo(0, row))?;
        stdout.queue(SetBackgroundColor(BG_WHITE))?;
        write!(stdout, "{}", " ".repeat(term_width.saturating_sub(1) as usize))?;
        stdout.queue(ResetColor)?;
    }

    let bot_row = start_row + 1 + max_text_rows as u16;
    if bot_row < term_height {
        paint_divider(stdout, bot_row, term_width)?;
    }

    // Place the cursor
    let vis_row = cursor_visual_row.min(display_rows.saturating_sub(1));
    let cursor_row = start_row + 1 + vis_row as u16;
    let visible_line = visual_lines.get(vis_row).cloned().unwrap_or_default();
    let col_offset: usize = visible_line
        .chars()
        .take(cursor_col_in_row)
        .map(char_width)
        .sum();
    let cursor_col = prefix_w + col_offset;
    let max_col = term_width.saturating_sub(1);
    stdout.queue(MoveTo(cursor_col.min(max_col as usize) as u16, cursor_row))?;
    stdout.queue(cursor::Show)?;

    stdout.flush()?;
    Ok(())
}

// ── LLM call ───────────────────────────────────────────────────────

fn make_system_prompt(mode: &str) -> String {
    match mode {
        "ask" => "You are HyperAgent — a universal AI assistant. Answer concisely.\n\
                  Format code with ```language```.\n\
                  Answer in the same language as the question."
            .into(),
        "general" => "You are HyperAgent — a versatile AI agent capable of ANY task.\n\
                      You handle: coding, research, writing, data analysis, and more.\n\
                      Format code with ```language```.\n\
                      Answer in the same language as the question."
            .into(),
        "code" => "You are HyperAgent in code mode. Answer coding questions.\n\
                   Format code with ```language```."
            .into(),
        "debug" => "You are HyperAgent in debug mode. Root-cause focused.\n\
                    Format code with ```language```."
            .into(),
        "architect" => "You are HyperAgent in architect mode. Design-focused.\n\
                        Discuss trade-offs. Answer in the user's language."
            .into(),
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

// ── Input handling ─────────────────────────────────────────────────

fn handle_key(app: &mut AppState, key: KeyEvent) {
    // Global shortcuts first
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('C'), KeyModifiers::CONTROL) => {
            app.should_exit = true;
            return;
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::Char('D'), KeyModifiers::CONTROL) => {
            app.should_exit = true;
            return;
        }
        _ => {}
    }

    match key.code {
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.insert_char('\n');
            } else {
                let prompt = app.submit_prompt();
                if prompt.is_empty() {
                    return;
                }
                handle_submit(app, prompt);
            }
        }
        KeyCode::Backspace => app.delete_backward(),
        KeyCode::Delete => app.delete_forward(),
        KeyCode::Left => app.move_left(),
        KeyCode::Right => app.move_right(),
        KeyCode::Home => app.move_home(),
        KeyCode::End => app.move_end(),
        KeyCode::Up => app.navigate_input_history(1),
        KeyCode::Down => app.navigate_input_history(-1),
        KeyCode::Esc => app.clear_input(),
        KeyCode::Char('a') | KeyCode::Char('A')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.move_home()
        }
        KeyCode::Char('e') | KeyCode::Char('E')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.move_end()
        }
        KeyCode::Char('k') | KeyCode::Char('K')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.delete_to_end()
        }
        KeyCode::Char('u') | KeyCode::Char('U')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.clear_input()
        }
        KeyCode::Char(c) => {
            // Ignore other Ctrl-/Alt-modified chars
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
            {
                app.insert_char(c);
            }
        }
        _ => {}
    }
}

fn handle_submit(app: &mut AppState, prompt: String) {
    if prompt.starts_with('/') {
        let cmd = prompt.trim();
        match cmd {
            "/exit" | "/quit" | "/q" => {
                app.should_exit = true;
                return;
            }
            "/help" | "/h" => {
                app.history.push((
                    "Help".into(),
                    "HyperAgent — Universal AI Agent\n\n\
                     /exit, /quit       Exit\n\
                     /mode <mode>       Switch mode (general/ask/code/debug/architect)\n\
                     /clear             Clear history\n\
                     /code <prompt>     Force code path\n\
                     /help              Show this help\n\n\
                     Editing\n\
                     ←/→, Home/End      Move cursor\n\
                     Backspace / Delete Delete char (left/right of cursor)\n\
                     Ctrl+A / Ctrl+E    Jump to start / end\n\
                     Ctrl+K             Delete to end\n\
                     Ctrl+U / Esc       Clear input\n\
                     Enter              Submit\n\
                     Shift+Enter        Newline\n\
                     ↑/↓                Input history\n\
                     Ctrl+C / Ctrl+D    Exit"
                        .into(),
                ));
                print_last_entry(&app.history);
                return;
            }
            "/clear" | "/cls" => {
                app.history.clear();
                println!("\x1b[2J\x1b[H"); // clear screen, home cursor
                return;
            }
            "/mode" => {
                app.history
                    .push(("Info".into(), format!("Current mode: {}", app.mode)));
                print_last_entry(&app.history);
                return;
            }
            c if c.starts_with("/mode ") => {
                let m = c[6..].trim();
                let valid = ["general", "ask", "code", "debug", "architect"];
                if valid.contains(&m) {
                    app.mode = m.to_string();
                    app.history.push(("Info".into(), format!("Mode: {m}")));
                } else {
                    app.history.push((
                        "Info".into(),
                        format!("Unknown mode: {m}. Options: {}", valid.join(", ")),
                    ));
                }
                print_last_entry(&app.history);
                return;
            }
            c if c.starts_with("/code ") => {
                let code_prompt = c[6..].trim().to_string();
                app.history.push((code_prompt.clone(), String::new()));
                print_user_prompt(&code_prompt);
                app.is_processing = true;
                let p = app.provider.clone();
                let mode = app.mode.clone();
                tokio::spawn(async move {
                    let resp = call_llm(&code_prompt, &[], &p, &mode).await;
                    llm_tx().send(resp).ok();
                });
                return;
            }
            _ => {
                app.history
                    .push(("Info".into(), format!("Unknown: {cmd}. /help")));
                print_last_entry(&app.history);
                return;
            }
        }
    }

    // Normal prompt → LLM
    app.history.push((prompt.clone(), String::new()));
    print_user_prompt(&prompt);
    app.is_processing = true;
    let p = app.provider.clone();
    let h = app.history[..app.history.len() - 1].to_vec();
    let mode = app.mode.clone();
    tokio::spawn(async move {
        let resp = call_llm(&prompt, &h, &p, &mode).await;
        llm_tx().send(resp).ok();
    });
}

fn print_user_prompt(prompt: &str) {
    println!();
    println!("  \x1b[34m❯\x1b[0m {prompt}");
}

fn print_last_entry(history: &[(String, String)]) {
    if let Some((user, assistant)) = history.last() {
        println!();
        println!("  \x1b[34m❯\x1b[0m {user}");
        for line in assistant.lines() {
            println!("  {line}");
        }
    }
}

// ── Main REPL ──────────────────────────────────────────────────────

pub async fn run_repl_tui() -> Result<()> {
    let provider = get_provider_from_config();
    let mut app = AppState::new(provider);

    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    stdout.queue(cursor::Hide)?;
    stdout.flush()?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let _ = LLM_TX.set(tx);

    println!();
    println!(
        "  \x1b[34mHyperAgent\x1b[0m — \x1b[32m{}\x1b[0m · \x1b[33m{}\x1b[0m",
        app.mode, app.provider.model
    );
    println!(
        "  \x1b[90m直接输入提问，或 /help 查看命令。Shift+Enter 换行，Ctrl+C 退出。\x1b[0m"
    );
    println!();

    loop {
        if app.should_exit {
            break;
        }

        let (columns, rows) = terminal::size()?;
        let ih = input_area_height(&app.input, columns);
        let prompt_start_row = rows.saturating_sub(ih);

        if app.is_processing {
            if let Ok(response) = rx.try_recv() {
                app.is_processing = false;
                if let Some((_, slot)) = app.history.last_mut() {
                    *slot = response;
                }
                if let Some(entry) = app.history.last() {
                    println!("  \x1b[34m❯\x1b[0m {}", entry.0);
                    for line in entry.1.lines() {
                        println!("  {line}");
                    }
                }
            }
        }

        draw_input_area(
            &mut stdout,
            &app.input,
            app.cursor_char,
            columns,
            rows,
            prompt_start_row,
        )?;

        if event::poll(Duration::from_millis(TICK_MS))? {
            if let Event::Key(key) = event::read()? {
                handle_key(&mut app, key);
            }
        }
    }

    stdout.queue(ResetColor)?;
    stdout.queue(cursor::Show)?;
    stdout.flush()?;
    terminal::disable_raw_mode()?;

    Ok(())
}
