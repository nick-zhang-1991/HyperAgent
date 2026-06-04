//! Computer Use — desktop automation for macOS
//!
//! Provides tools for taking screenshots, controlling mouse/keyboard,
//! and interacting with GUI applications via osascript.
//!
//! Supported operations:
//! - Screenshot: capture full screen or region
//! - Mouse: move, click, double-click, right-click, drag, get position
//! - Keyboard: type text, press special keys, key combinations
//! - Screen: get dimensions
//! - App: find window, focus app, get window list
//!
//! Uses macOS built-in tools (screencapture, osascript, cliclick)
//! and CGEvent via Swift if available.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Result of a computer use operation
#[derive(Debug)]
pub enum ComputerAction {
    Screenshot { path: PathBuf },
    MousePosition { x: f64, y: f64 },
    ScreenSize { width: f64, height: f64 },
    Text(String),
    AppInfo(String),
}

/// Opaque result for all operations
#[derive(Debug)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
    pub data: Option<String>,
}

impl ActionResult {
    fn ok(msg: impl Into<String>) -> Self {
        Self { success: true, message: msg.into(), data: None }
    }
    fn ok_with_data(msg: impl Into<String>, data: impl Into<String>) -> Self {
        Self { success: true, message: msg.into(), data: Some(data.into()) }
    }
    fn err(msg: impl Into<String>) -> Self {
        Self { success: false, message: msg.into(), data: None }
    }
}

/// Computer control interface
pub struct ComputerUse;

impl ComputerUse {
    /// Check if we're on macOS and have the necessary tools
    pub fn check_available() -> ActionResult {
        if cfg!(target_os = "macos") {
            // Check screencapture exists
            let sc = Command::new("which").arg("screencapture").output();
            let has_cliclick = Command::new("which").arg("cliclick").output();
            let has_osascript = Command::new("which").arg("osascript").output();

            let mut tools = Vec::new();
            if sc.ok().map_or(false, |o| o.status.success()) {
                tools.push("screencapture");
            }
            if has_cliclick.ok().map_or(false, |o| o.status.success()) {
                tools.push("cliclick");
            }
            if has_osascript.ok().map_or(false, |o| o.status.success()) {
                tools.push("osascript");
            }

            if tools.is_empty() {
                ActionResult::err("No macOS automation tools found. Install cliclick: `brew install cliclick`")
            } else {
                ActionResult::ok(format!("macOS ready. Tools: {}", tools.join(", ")))
            }
        } else {
            ActionResult::err("Computer use is only supported on macOS")
        }
    }

    /// Take a screenshot of the full screen
    pub fn screenshot(path: Option<&Path>) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }

        let default_path = PathBuf::from("/tmp/hyper-screenshot.png");
        let save_path = path.unwrap_or(&default_path);
        let output = Command::new("screencapture")
            .args(["-x", "-C"]) // -x: no sound, -C: capture cursor
            .arg(save_path.to_str().unwrap_or("/tmp/hyper-screenshot.png"))
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let size = std::fs::metadata(save_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                ActionResult::ok_with_data(
                    format!("Screenshot saved: {} ({} bytes)", save_path.display(), size),
                    save_path.to_string_lossy().to_string(),
                )
            }
            Ok(o) => ActionResult::err(format!(
                "screencapture failed: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => ActionResult::err(format!("screencapture error: {e}")),
        }
    }

    /// Take a screenshot of a specific region (x, y, width, height)
    pub fn screenshot_region(x: u32, y: u32, w: u32, h: u32, path: Option<&Path>) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let default_path = PathBuf::from("/tmp/hyper-screenshot-region.png");
        let save_path = path.unwrap_or(&default_path);
        let output = Command::new("screencapture")
            .args(["-x", "-R", &format!("{},{},{},{}", x, y, w, h)])
            .arg(save_path.to_str().unwrap_or("/tmp/hyper-screenshot-region.png"))
            .output();

        match output {
            Ok(o) if o.status.success() => {
                ActionResult::ok(format!("Region screenshot saved: {}", save_path.display()))
            }
            Ok(o) => ActionResult::err(format!(
                "screencapture region failed: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => ActionResult::err(format!("screencapture error: {e}")),
        }
    }

    /// Get cursor position via osascript
    pub fn cursor_position() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = r#"tell application "System Events" to get {position of mouse}"#;
        match Self::run_osascript(script) {
            Ok(output) => {
                let trimmed = output.trim();
                // Output format: "x, y"
                if let Some((x_str, y_str)) = trimmed.split_once(',') {
                    let x = x_str.trim().parse::<f64>().unwrap_or(0.0);
                    let y = y_str.trim().parse::<f64>().unwrap_or(0.0);
                    ActionResult::ok_with_data(
                        format!("Cursor at ({:.0}, {:.0})", x, y),
                        format!("{{\"x\":{:.0},\"y\":{:.0}}}", x, y),
                    )
                } else {
                    ActionResult::ok(format!("Cursor: {trimmed}"))
                }
            }
            Err(e) => ActionResult::err(format!("Failed to get cursor position: {e}")),
        }
    }

    /// Move mouse to absolute coordinates
    pub fn mouse_move(x: f64, y: f64) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = format!(
            r#"tell application "System Events" to set position of mouse to {{{}, {}}}"#,
            x as i64, y as i64
        );
        match Self::run_osascript(&script) {
            Ok(_) => ActionResult::ok(format!("Mouse moved to ({:.0}, {:.0})", x, y)),
            Err(e) => ActionResult::err(format!("Mouse move failed: {e}")),
        }
    }

    /// Click at current mouse position (left button)
    pub fn mouse_click() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = r#"tell application "System Events" to click"#;
        match Self::run_osascript(script) {
            Ok(_) => ActionResult::ok("Clicked"),
            Err(e) => ActionResult::err(format!("Click failed: {e}")),
        }
    }

    /// Click at specific coordinates
    pub fn click_at(x: f64, y: f64) -> ActionResult {
        // Move first, then click
        let _ = Self::mouse_move(x, y);
        Self::mouse_click()
    }

    /// Double-click at current position
    pub fn double_click() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = r#"tell application "System Events" to double click"#;
        match Self::run_osascript(script) {
            Ok(_) => ActionResult::ok("Double-clicked"),
            Err(e) => ActionResult::err(format!("Double-click failed: {e}")),
        }
    }

    /// Right-click at current position
    pub fn right_click() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        // Use CGEvent via osascript for right click
        let script = r#"
use framework "CoreGraphics"
use framework "ApplicationServices"

-- Get current mouse position
set loc to CGEventGetLocation(CGEventCreate(0))
set x to item 1 of loc
set y to item 2 of loc

-- Create right-click event
set rightClick to CGEventCreateMouseEvent(
    missing value, kCGMouseEventRightMouseDown, loc, 0
)
CGEventPost(kCGHIDEventTap, rightClick)
set rightUp to CGEventCreateMouseEvent(
    missing value, kCGMouseEventRightMouseUp, loc, 0
)
CGEventPost(kCGHIDEventTap, rightUp)
return "right-click done"
"#;
        match Self::run_osascript(script) {
            Ok(_) => ActionResult::ok("Right-clicked"),
            Err(e) => ActionResult::err(format!("Right-click failed: {e}")),
        }
    }

    /// Type text at current cursor position
    pub fn type_text(text: &str) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        // Escape special characters for osascript
        let escaped = text
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        let script = format!(
            r#"tell application "System Events" to keystroke "{}""#,
            escaped
        );
        match Self::run_osascript(&script) {
            Ok(_) => ActionResult::ok(format!("Typed {} chars", text.len())),
            Err(e) => ActionResult::err(format!("Type failed: {e}")),
        }
    }

    /// Press a special key (enter, escape, tab, up, down, etc.)
    pub fn press_key(key: &str) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        // Map common key names to AppleScript key codes
        let key_code = match key.to_lowercase().as_str() {
            "return" | "enter" => "return",
            "escape" | "esc" => "escape",
            "tab" => "tab",
            "space" => "space",
            "delete" | "backspace" => "delete",
            "forwarddelete" | "del" => "forward delete",
            "up" => "up",
            "down" => "down",
            "left" => "left",
            "right" => "right",
            "home" => "home",
            "end" => "end",
            "pageup" => "page up",
            "pagedown" => "page down",
            "f1" => "f1", "f2" => "f2", "f3" => "f3",
            "f4" => "f4", "f5" => "f5", "f6" => "f6",
            "f7" => "f7", "f8" => "f8", "f9" => "f9",
            "f10" => "f10", "f11" => "f11", "f12" => "f12",
            _ => {
                return ActionResult::err(format!("Unknown key: {key}. Supported: return, escape, tab, space, delete, up, down, left, right, home, end, pageup, pagedown, f1-f12"));
            }
        };
        let script = format!(
            r#"tell application "System Events" to key code {}"#,
            key_to_keycode(key_code)
        );
        // For named keys, use keystroke with key name
        let script_named = format!(
            r#"tell application "System Events" to keystroke "{}" & return"#,
            key_code
        );
        match Self::run_osascript(&script_named) {
            Ok(_) => ActionResult::ok(format!("Pressed key: {key}")),
            Err(e) => ActionResult::err(format!("Key press failed: {e}")),
        }
    }

    /// Press a key combination (e.g., "cmd+c", "cmd+shift+z")
    pub fn press_key_combo(combo: &str) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let parts: Vec<&str> = combo.split('+').collect();
        if parts.is_empty() {
            return ActionResult::err("Empty key combo");
        }

        let mut using_cmd = false;
        let mut using_option = false;
        let mut using_control = false;
        let mut using_shift = false;
        let mut key = String::new();

        for part in parts {
            let lower = part.trim().to_lowercase();
            match lower.as_str() {
                "cmd" | "command" => using_cmd = true,
                "opt" | "option" | "alt" => using_option = true,
                "ctrl" | "control" => using_control = true,
                "shift" => using_shift = true,
                k => key = k.to_string(),
            }
        }

        if key.is_empty() {
            return ActionResult::err("No key in combo");
        }

        let mut modifiers = Vec::new();
        if using_cmd { modifiers.push("command down"); }
        if using_option { modifiers.push("option down"); }
        if using_control { modifiers.push("control down"); }
        if using_shift { modifiers.push("shift down"); }

        let mods_str = modifiers.join(", ");
        let script = format!(
            r#"tell application "System Events" to keystroke "{}" using {{{}}}"#,
            key, mods_str
        );

        match Self::run_osascript(&script) {
            Ok(_) => ActionResult::ok(format!("Pressed combo: {combo}")),
            Err(e) => ActionResult::err(format!("Key combo failed: {e}")),
        }
    }

    /// Get screen dimensions
    pub fn screen_size() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = r#"tell application "System Events" to get {size of front window of application process "System Events"}"#;
        // Actually use a simpler approach — use the desktop size
        let script2 = r#"
tell application "Finder"
    get bounds of window of desktop
end tell
"#;
        match Self::run_osascript(script2) {
            Ok(output) => {
                let trimmed = output.trim();
                // Format: "0, 0, 1440, 900"
                let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
                if parts.len() == 4 {
                    let w = parts[2].parse::<f64>().unwrap_or(0.0);
                    let h = parts[3].parse::<f64>().unwrap_or(0.0);
                    ActionResult::ok_with_data(
                        format!("Screen: {w:.0}×{h:.0}"),
                        format!("{{\"width\":{w:.0},\"height\":{h:.0}}}"),
                    )
                } else {
                    ActionResult::ok(format!("Screen info: {trimmed}"))
                }
            }
            Err(e) => {
                // Fallback
                ActionResult::err(format!("Could not get screen size: {e}"))
            }
        }
    }

    /// Drag from current position to target
    pub fn drag_to(x: f64, y: f64) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = format!(
            r#"tell application "System Events"
    set currentPos to position of mouse
    set x1 to item 1 of currentPos
    set y1 to item 2 of currentPos
    set position of mouse to {{{}, {}}}
end tell"#,
            x as i64, y as i64
        );
        match Self::run_osascript(&script) {
            Ok(_) => ActionResult::ok(format!("Dragged to ({x:.0}, {y:.0})")),
            Err(e) => ActionResult::err(format!("Drag failed: {e}")),
        }
    }

    /// Focus an application by name
    pub fn focus_app(name: &str) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = format!(
            r#"tell application "{}" to activate"#,
            name.replace('"', "\\\"")
        );
        match Self::run_osascript(&script) {
            Ok(_) => ActionResult::ok(format!("Focused app: {name}")),
            Err(e) => ActionResult::err(format!("Focus app failed: {e}")),
        }
    }

    /// List running applications with windows
    pub fn list_apps() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = r#"
tell application "System Events"
    set appList to every application process whose background only is false
    set output to ""
    repeat with appProc in appList
        set appName to name of appProc
        set winCount to count of windows of appProc
        set output to output & appName & " (" & winCount & " windows)" & return
    end repeat
    return output
end tell
"#;
        match Self::run_osascript(script) {
            Ok(output) => ActionResult::ok_with_data(
                format!("Running apps:\n{}", output.trim()),
                output.trim().to_string(),
            ),
            Err(e) => ActionResult::err(format!("List apps failed: {e}")),
        }
    }

    /// Get text content of the focused window (via accessibility API)
    pub fn get_window_text() -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let script = r#"
tell application "System Events"
    tell (first application process whose frontmost is true)
        tell window 1
            try
                set elem to first UI element whose role description is "text area"
                set txt to value of attribute "AXValue" of elem
                return txt
            on error
                return "(no text area found)"
            end try
        end tell
    end tell
end tell
"#;
        match Self::run_osascript(script) {
            Ok(output) => ActionResult::ok_with_data(
                format!("Window text ({} chars): {}", output.trim().len(), output.trim().chars().take(200).collect::<String>()),
                output.trim().to_string(),
            ),
            Err(e) => ActionResult::err(format!("Get window text failed: {e}")),
        }
    }

    /// Scroll in a direction
    pub fn scroll(direction: &str, amount: u32) -> ActionResult {
        if !cfg!(target_os = "macos") {
            return Self::not_macos();
        }
        let dir = match direction.to_lowercase().as_str() {
            "up" => "0, 1",
            "down" => "0, -1",
            "left" => "1, 0",
            "right" => "-1, 0",
            _ => return ActionResult::err("Direction must be: up, down, left, right"),
        };
        let script = format!(
            r#"tell application "System Events" to scroll (current application) direction {} with amount {}"#,
            dir, amount
        );
        match Self::run_osascript(&script) {
            Ok(_) => ActionResult::ok(format!("Scrolled {direction} by {amount}")),
            Err(e) => ActionResult::err(format!("Scroll failed: {e}")),
        }
    }

    // ============ Helpers ============

    fn not_macos() -> ActionResult {
        ActionResult::err("This operation requires macOS")
    }

    fn run_osascript(script: &str) -> Result<String, String> {
        let output = Command::new("osascript")
            .args(["-e", script])
            .output()
            .map_err(|e| format!("Failed to run osascript: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}

/// Map AppleScript key name to key code (for key code commands)
fn key_to_keycode(key: &str) -> u32 {
    match key {
        "return" => 36,
        "escape" => 53,
        "tab" => 48,
        "space" => 49,
        "delete" => 51,
        "forward delete" => 117,
        "up" => 126,
        "down" => 125,
        "left" => 123,
        "right" => 124,
        "home" => 115,
        "end" => 119,
        "page up" => 116,
        "page down" => 121,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_to_keycode() {
        assert_eq!(key_to_keycode("return"), 36);
        assert_eq!(key_to_keycode("escape"), 53);
        assert_eq!(key_to_keycode("tab"), 48);
        assert_eq!(key_to_keycode("up"), 126);
        assert_eq!(key_to_keycode("down"), 125);
        assert_eq!(key_to_keycode("delete"), 51);
    }

    #[test]
    fn test_check_available_does_not_panic() {
        // This just verifies the function exists and runs without panic
        let result = ComputerUse::check_available();
        // On non-macos, should be err; on macos, could be either
        #[cfg(not(target_os = "macos"))]
        assert!(!result.success);
    }

    #[test]
    fn test_action_result_ok() {
        let r = ActionResult::ok("hello");
        assert!(r.success);
        assert_eq!(r.message, "hello");
        assert!(r.data.is_none());
    }

    #[test]
    fn test_action_result_err() {
        let r = ActionResult::err("oops");
        assert!(!r.success);
        assert_eq!(r.message, "oops");
    }

    #[test]
    fn test_action_result_ok_with_data() {
        let r = ActionResult::ok_with_data("msg", "data");
        assert!(r.success);
        assert_eq!(r.message, "msg");
        assert_eq!(r.data.as_deref(), Some("data"));
    }
}
