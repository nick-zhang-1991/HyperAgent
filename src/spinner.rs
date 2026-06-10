//! Simple progress spinner for terminal feedback during LLM calls
//!
//! Cross-platform: uses Unicode braille on modern terminals (macOS, Linux, Windows 10+),
//! falls back to ASCII spinner on legacy Windows consoles.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A simple terminal spinner that runs on a background thread.
/// Drop the handle to stop the spinner.
#[allow(dead_code)]
pub struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    #[allow(dead_code)]
    /// Start a new spinner with the given message
    pub fn start(message: impl Into<String>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let msg: String = message.into();
        let display_msg = msg.clone();

        // Detect if we need ASCII fallback (legacy Windows console without ANSI support)
        let use_ascii = is_legacy_windows_console();

        let handle = thread::spawn(move || {
            let frames_unicode = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let frames_ascii = ["-", "\\", "|", "/"];
            let frames = if use_ascii { &frames_ascii[..] } else { &frames_unicode[..] };
            let mut i = 0;
            while r.load(Ordering::Relaxed) {
                let frame = frames[i % frames.len()];
                print!("\r {} {}... ", frame, display_msg);
                use std::io::{Write, stdout};
                let _ = stdout().flush();
                thread::sleep(Duration::from_millis(if use_ascii { 120 } else { 80 }));
                i += 1;
            }
            // Clear the spinner line
            print!("\r\x1b[K");
            use std::io::{Write, stdout};
            let _ = stdout().flush();
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    /// Stop the spinner and return the message
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Detect legacy Windows console (cmd.exe) that lacks ANSI escape support.
/// Windows 10+ with Windows Terminal, PowerShell, or ConEmu all support ANSI.
/// Only true legacy cmd.exe on Windows 8 or older needs fallback.
fn is_legacy_windows_console() -> bool {
    #[cfg(windows)]
    {
        // Check for modern terminals that support ANSI
        if std::env::var("WT_SESSION").is_ok()     // Windows Terminal
            || std::env::var("TERM_PROGRAM").is_ok() // Various terminals
            || std::env::var("ANSICON").is_ok()      // ANSICON loaded
        {
            return false;
        }
        // Check if we're on a modern-enough Windows (10+)
        // Simple heuristic: check for Powershell 5+ or newer cmd
        if let Ok(ver) = std::env::var("PROCESSOR_ARCHITECTURE") {
            // On 64-bit Windows we're likely on a modern system
            if ver == "AMD64" || ver == "ARM64" {
                return false;
            }
        }
        true // Assume legacy if unsure
    }
    #[cfg(not(windows))]
    {
        false // Unix always supports ANSI
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_spinner_start_and_drop() {
        // start a spinner, then let it drop (Drop::stop should run cleanly)
        {
            let _s = Spinner::start("loading");
            // give the thread a moment to start
            std::thread::sleep(Duration::from_millis(50));
        }
        // If Drop impl is correct, this returns without hanging or panic.
    }

    #[test]
    fn test_spinner_explicit_stop_returns_quickly() {
        let mut s = Spinner::start("working");
        let t0 = Instant::now();
        s.stop();
        let elapsed = t0.elapsed();
        // stop should join the thread in <1s
        assert!(elapsed < Duration::from_secs(2),
                "stop() took too long: {:?}", elapsed);
    }

    #[test]
    fn test_spinner_double_stop_is_safe() {
        let mut s = Spinner::start("test");
        s.stop();
        // second stop should be a no-op (handle is None)
        s.stop();
    }

    #[test]
    fn test_spinner_accepts_string_and_str() {
        // Verify Into<String> works for both &str and String
        let s1 = Spinner::start("from str");
        drop(s1);
        let owned = String::from("from String");
        let s2 = Spinner::start(owned);
        drop(s2);
    }

    #[test]
    fn test_spinner_runs_multiple_concurrently() {
        // Verify multiple spinners don't conflict
        let mut a = Spinner::start("task a");
        let mut b = Spinner::start("task b");
        std::thread::sleep(Duration::from_millis(50));
        a.stop();
        b.stop();
    }

    #[test]
    fn test_is_legacy_windows_console_returns_false_on_unix() {
        // On non-Windows, this should always return false.
        #[cfg(not(windows))]
        assert!(!is_legacy_windows_console());
    }
}
