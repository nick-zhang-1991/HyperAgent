//! Background Process Manager — spawn, monitor, and control long-running processes
//!
//! Supports:
//! - Spawn processes with arg lists and working directory
//! - Poll output without blocking
//! - Send stdin input
//! - Kill by session ID
//! - List all running processes

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// A single background process handle
#[derive(Debug)]
pub struct BgProcess {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub started_at: Instant,
    pub status: BgStatus,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    output: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BgStatus {
    Running,
    Done(i32),
    Killed,
    Failed(String),
}

impl BgProcess {
    fn new(id: String, command: String, args: Vec<String>, cwd: String) -> Self {
        Self {
            id,
            command,
            args,
            cwd,
            started_at: Instant::now(),
            status: BgStatus::Running,
            child: None,
            stdin: None,
            output: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// Manages all background processes
#[derive(Debug, Default)]
pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, BgProcess>>>,
    counter: Arc<Mutex<u64>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a command in the background
    pub fn spawn(&self, command: &str, args: &[String], cwd: &str) -> Result<String, String> {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let id = format!("bg-{}", counter);

        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn: {e}"))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let mut process = BgProcess::new(
            id.clone(),
            command.to_string(),
            args.to_vec(),
            cwd.to_string(),
        );

        // Spawn reader threads that capture output into the shared buffer
        let output = process.output.clone();

        if let Some(stdout) = stdout {
            let out = output.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if let Ok(mut buf) = out.lock() {
                            buf.push(line);
                        }
                    }
                }
            });
        }

        if let Some(stderr) = stderr {
            let out = output.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if let Ok(mut buf) = out.lock() {
                            buf.push(format!("[stderr] {line}"));
                        }
                    }
                }
            });
        }

        process.child = Some(child);
        process.stdin = stdin;

        let mut processes = self.processes.lock().unwrap();
        processes.insert(id.clone(), process);

        Ok(id)
    }

    /// Poll and update process status. Call before reading output.
    pub fn poll(&self, id: &str) -> Option<BgStatus> {
        let mut processes = self.processes.lock().unwrap();
        let process = processes.get_mut(id)?;
        if process.status != BgStatus::Running {
            return Some(process.status.clone());
        }
        if let Some(ref mut child) = process.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(-1);
                    if exit_code == 0 {
                        process.status = BgStatus::Done(exit_code);
                    } else {
                        process.status = if exit_code == -9 || exit_code == -15 {
                            BgStatus::Killed
                        } else {
                            BgStatus::Done(exit_code)
                        };
                    }
                    Some(process.status.clone())
                }
                Ok(None) => Some(BgStatus::Running),
                Err(e) => {
                    process.status = BgStatus::Failed(format!("{e}"));
                    Some(process.status.clone())
                }
            }
        } else {
            None
        }
    }

    /// Read buffered output since last check
    pub fn read_output(&self, id: &str) -> Vec<String> {
        let mut processes = self.processes.lock().unwrap();
        if let Some(process) = processes.get(id) {
            if let Ok(mut buf) = process.output.lock() {
                let lines = buf.clone();
                buf.clear();
                return lines;
            }
        }
        vec![]
    }

    /// Get all output (non-destructive)
    pub fn all_output(&self, id: &str) -> Vec<String> {
        let processes = self.processes.lock().unwrap();
        if let Some(process) = processes.get(id) {
            if let Ok(buf) = process.output.lock() {
                return buf.clone();
            }
        }
        vec![]
    }

    /// Send text to stdin of a running process
    pub fn send_input(&self, id: &str, text: &str) -> Result<(), String> {
        let mut processes = self.processes.lock().unwrap();
        let process = processes
            .get_mut(id)
            .ok_or_else(|| format!("Process {id} not found"))?;
        if let Some(ref mut stdin) = process.stdin {
            writeln!(stdin, "{text}").map_err(|e| format!("Failed to write stdin: {e}"))?;
            let _ = stdin.flush();
            Ok(())
        } else {
            Err("Process has no stdin".to_string())
        }
    }

    /// Kill a background process
    pub fn kill(&self, id: &str) -> Result<(), String> {
        let mut processes = self.processes.lock().unwrap();
        let process = processes
            .get_mut(id)
            .ok_or_else(|| format!("Process {id} not found"))?;
        if process.status != BgStatus::Running {
            return Err("Process is not running".to_string());
        }
        if let Some(ref mut child) = process.child {
            let _ = child.kill();
            let _ = child.wait();
            process.status = BgStatus::Killed;
            Ok(())
        } else {
            Err("No child handle".to_string())
        }
    }

    /// Wait for a process to complete (with timeout in seconds)
    pub fn wait(&self, id: &str, timeout_secs: u64) -> Result<BgStatus, String> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.poll(id) {
                match status {
                    BgStatus::Running => {
                        if start.elapsed().as_secs() >= timeout_secs {
                            return Ok(BgStatus::Running);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    _ => return Ok(status),
                }
            } else {
                return Err(format!("Process {id} not found"));
            }
        }
    }

    /// List all tracked processes
    pub fn list(&self) -> Vec<(String, String, BgStatus, String)> {
        let processes = self.processes.lock().unwrap();
        let mut result: Vec<_> = processes
            .iter()
            .map(|(id, p)| {
                let elapsed = p.started_at.elapsed();
                let elapsed_str = format!("{:.0}s", elapsed.as_secs_f64());
                let cmd = format!("{} {}", p.command, p.args.join(" "));
                (id.clone(), cmd, p.status.clone(), elapsed_str)
            })
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Clean up completed processes
    pub fn cleanup(&self) -> usize {
        let mut processes = self.processes.lock().unwrap();
        let mut removed = 0;
        let ids: Vec<String> = processes
            .iter()
            .filter(|(_, p)| p.status != BgStatus::Running)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            processes.remove(&id);
            removed += 1;
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_manager_new() {
        let pm = ProcessManager::new();
        assert!(pm.list().is_empty());
    }

    #[test]
    fn test_spawn_basic() {
        let pm = ProcessManager::new();
        let id = pm.spawn("echo", &["hello".to_string()], "/tmp").unwrap();
        assert!(id.starts_with("bg-"));
        // Brief wait for process to complete
        std::thread::sleep(std::time::Duration::from_millis(100));
        // Process should have completed
        assert_eq!(pm.list().len(), 1);
    }

    #[test]
    fn test_spawn_nonexistent_cmd() {
        let pm = ProcessManager::new();
        let result = pm.spawn("nonexistent-command-xyz", &[], "/tmp");
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_unknown() {
        let pm = ProcessManager::new();
        pm.kill("ghost-id");
        // Should not panic
    }

    #[test]
    fn test_cleanup_empty() {
        let pm = ProcessManager::new();
        assert_eq!(pm.cleanup(), 0);
    }

    #[test]
    fn test_read_output_unknown() {
        let pm = ProcessManager::new();
        let output = pm.read_output("ghost");
        assert!(output.is_empty());
    }

    #[test]
    fn test_bg_status_serialization() {
        let status = BgStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"Running\"");

        let parsed: BgStatus = serde_json::from_str("\"Running\"").unwrap();
        assert_eq!(parsed, BgStatus::Running);

        let parsed: BgStatus = serde_json::from_str("\"Killed\"").unwrap();
        assert_eq!(parsed, BgStatus::Killed);
    }
}
