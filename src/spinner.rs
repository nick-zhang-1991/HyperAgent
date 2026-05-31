//! Simple progress spinner for terminal feedback during LLM calls

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

        let handle = thread::spawn(move || {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0;
            while r.load(Ordering::Relaxed) {
                let frame = frames[i % frames.len()];
                print!("\r {} {}... ", frame, display_msg);
                use std::io::{Write, stdout};
                stdout().flush().ok();
                thread::sleep(Duration::from_millis(80));
                i += 1;
            }
            // Clear the spinner line
            print!("\r\x1b[K");
            use std::io::{Write, stdout};
            stdout().flush().ok();
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
