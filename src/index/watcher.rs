use anyhow::Result;
use notify::Config;
use notify::Event;
use notify::EventKind;
use notify::RecommendedWatcher;
use notify::Watcher;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Watches the filesystem for changes and triggers re-indexing
#[allow(dead_code)]
pub struct FileWatcher {
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
    #[allow(dead_code)]
    running: Arc<AtomicBool>,
}

impl FileWatcher {
    #[allow(dead_code)]
    /// Create a new file watcher
    pub fn new(_root: &Path, callback: impl Fn(Vec<String>) + Send + 'static) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Event>();
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default(),
        )?;

        // Spawn processing thread
        thread::spawn(move || {
            use std::time::{Duration, Instant};
            let mut last_trigger = Instant::now();
            let mut pending: Vec<String> = Vec::new();

            while r.load(Ordering::Relaxed) {
                if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
                    // Collect changed files
                    if matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) {
                        for path in event.paths {
                            if let Some(p) = path.to_str() {
                                pending.push(p.to_string());
                            }
                        }
                    }

                    // Debounce: trigger callback after 1s of no changes
                    if last_trigger.elapsed() > Duration::from_secs(1) && !pending.is_empty() {
                        callback(std::mem::take(&mut pending));
                        last_trigger = Instant::now();
                    }
                } else {
                    // Timeout - check if we have pending changes
                    if last_trigger.elapsed() > Duration::from_secs(1) && !pending.is_empty() {
                        callback(std::mem::take(&mut pending));
                        last_trigger = Instant::now();
                    }
                }
            }
        });

        Ok(Self {
            watcher,
            running,
        })
    }

    #[allow(dead_code)]
    #[allow(dead_code)]
    /// Start watching
    pub fn watch(&mut self, path: &Path) -> Result<()> {
        self.watcher
            .watch(path, notify::RecursiveMode::Recursive)?;
        Ok(())
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
