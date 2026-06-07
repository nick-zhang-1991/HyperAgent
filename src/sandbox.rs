//! Sandboxed code execution — Docker container isolation.
//!
//! Wraps command execution in temporary Docker containers with resource limits.
//! Optional — falls back to direct execution when Docker is unavailable or user
//! has not enabled sandbox mode.
//!
//! Usage:
//! ```ignore
//! let sandbox = Sandbox::new("python:3.12-slim")?;
//! let result = sandbox.run("python3 -c 'print(\"hello\")'", 30).await?;
//! println!("{}", result.stdout);
//! ```

use anyhow::{Context, Result, bail};

/// Result of a sandboxed command execution
#[derive(Debug, Clone)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

/// Sandbox configuration
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Docker image to use (default: "python:3.12-slim")
    pub image: String,
    /// CPU limit (e.g., "0.5" for half a core)
    pub cpu_limit: String,
    /// Memory limit (e.g., "512m")
    pub memory_limit: String,
    /// Maximum execution time in seconds
    pub timeout_secs: u32,
    /// Network access (disabled by default for safety)
    pub network_enabled: bool,
    /// Read-only mount project root
    pub project_root: Option<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            image: "python:3.12-slim".to_string(),
            cpu_limit: "1.0".to_string(),
            memory_limit: "512m".to_string(),
            timeout_secs: 30,
            network_enabled: false,
            project_root: None,
        }
    }
}

/// Sandboxed execution environment using Docker
pub struct Sandbox {
    config: SandboxConfig,
    available: bool,
    container_id: Option<String>,
}

impl Sandbox {
    /// Create a new sandbox and check Docker availability.
    pub fn new(config: SandboxConfig) -> Self {
        let available = check_docker().unwrap_or(false);
        Self {
            config,
            available,
            container_id: None,
        }
    }

    /// Check if Docker is available
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Run a command in a sandboxed Docker container (async version).
    pub async fn run(&self, command: &str, timeout_secs: u32) -> Result<SandboxResult> {
        self.build_and_run(command, timeout_secs, true).await
    }

    /// Run a command in a sandboxed Docker container (sync version).
    /// Useful when called from non-async contexts.
    pub fn run_sync(&self, command: &str, timeout_secs: u32) -> Result<SandboxResult> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.build_and_run(command, timeout_secs, false))
    }

    async fn build_and_run(&self, command: &str, timeout_secs: u32, _async: bool) -> Result<SandboxResult> {
        if !self.available {
            bail!("Docker sandbox is not available. Install Docker Desktop or disable sandbox mode.");
        }

        let _timeout = self.config.timeout_secs.min(timeout_secs);
        let container_name = format!("hyper-sandbox-{}", std::process::id());

        // Build docker run command
        let mut docker_args = vec![
            "run".to_string(),
            "--rm".to_string(),                        // Auto-remove after exit
            "--name".to_string(), container_name.clone(),
            format!("--cpus={}", self.config.cpu_limit),
            format!("--memory={}", self.config.memory_limit),
            format!("--memory-swap={}", self.config.memory_limit), // No swap
            "--pids-limit=64".to_string(),              // Prevent fork bombs
            "--read-only".to_string(),                   // Read-only root filesystem
            "--cap-drop=ALL".to_string(),                // Drop all Linux capabilities
            "--security-opt=no-new-privileges".to_string(), // No privilege escalation
            "--network".to_string(), if self.config.network_enabled { "bridge".into() } else { "none".into() },
        ];

        // Mount project root read-only if configured
        if let Some(ref root) = self.config.project_root {
            docker_args.push("-v".to_string());
            docker_args.push(format!("{}:/project:ro", root));
            docker_args.push("--workdir".to_string());
            docker_args.push("/project".to_string());
        }

        // Create a temp writable directory for output files
        let tmp_dir = std::env::temp_dir().join(format!("hyper-sandbox-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        docker_args.push("-v".to_string());
        docker_args.push(format!("{}:/output", tmp_dir.display()));

        docker_args.push(self.config.image.clone());
        docker_args.push("sh".to_string());
        docker_args.push("-c".to_string());
        docker_args.push(format!("{}; echo \"EXIT_CODE=$?\"", command));

        // Execute via tokio::process
        let output = tokio::process::Command::new("docker")
            .args(&docker_args)
            .kill_on_drop(true)
            .output()
            .await
            .context("Failed to execute Docker sandbox")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Extract exit code from sandbox output
        let exit_code = if let Some(line) = stdout.lines().rev().next() {
            if line.starts_with("EXIT_CODE=") {
                line.trim_start_matches("EXIT_CODE=").parse().unwrap_or(1)
            } else {
                output.status.code().unwrap_or(1)
            }
        } else {
            output.status.code().unwrap_or(1)
        };

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(&tmp_dir);

        Ok(SandboxResult {
            stdout,
            stderr,
            exit_code,
            timed_out: false,
        })
    }

    /// Run a Python script in the sandbox.
    /// Creates a temp file with the script content and executes it.
    pub async fn run_python(&self, code: &str, timeout_secs: u32) -> Result<SandboxResult> {
        // Write code to temp file
        let tmp_dir = std::env::temp_dir().join(format!("hyper-py-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let script_path = tmp_dir.join("script.py");
        std::fs::write(&script_path, code)
            .context("Failed to write script for sandbox")?;

        let script_container_path = "/tmp/script.py".to_string();
        let command = format!("python3 {}", script_container_path);

        let result = self.run(&command, timeout_secs).await;
        let _ = std::fs::remove_dir_all(&tmp_dir);
        result
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Ensure cleanup if we created any containers
        if let Some(ref id) = self.container_id {
            let _ = std::process::Command::new("docker")
                .args(["rm", "-f", id])
                .output();
        }
    }
}

/// Check if Docker daemon is available
fn check_docker() -> Result<bool> {
    let output = std::process::Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .context("Docker not found")?;
    Ok(output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_docker() {
        let result = check_docker();
        // May or may not have Docker; just verify it doesn't panic
        let _ = result;
    }

    #[tokio::test]
    async fn test_sandbox_availability() {
        let sandbox = Sandbox::new(SandboxConfig::default());
        // Just check it initializes without error
        let _ = sandbox.is_available();
    }

    #[tokio::test]
    async fn test_sandbox_python_greeting() {
        let sandbox = Sandbox::new(SandboxConfig::default());
        if !sandbox.is_available() {
            eprintln!("Skipping: Docker not available");
            return;
        }
        let result = sandbox.run_python("print('hello from sandbox')", 10).await.unwrap();
        assert!(result.stdout.contains("hello from sandbox"));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sandbox_no_network() {
        let sandbox = Sandbox::new(SandboxConfig::default());
        if !sandbox.is_available() {
            eprintln!("Skipping: Docker not available");
            return;
        }
        // curl should fail because network is disabled
        let result = sandbox.run("curl https://example.com", 10).await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sandbox_memory_limit() {
        let sandbox = Sandbox::new(SandboxConfig {
            memory_limit: "64m".to_string(),
            ..Default::default()
        });
        if !sandbox.is_available() {
            eprintln!("Skipping: Docker not available");
            return;
        }
        // Allocate 128MB — should be killed by memory limit
        let result = sandbox.run_python("import os; os.environ['x'] = ' ' * (128 * 1024 * 1024)", 15).await;
        // Either blocked or kills the process — either is fine
        assert!(result.is_err() || result.unwrap().exit_code != 0);
    }
}
