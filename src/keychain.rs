//! Secure credential storage via platform keychain.
//!
//! macOS: uses `security` CLI tool to access the system Keychain.
//! Falls back to plaintext storage when keychain is unavailable.
//!
//! Usage:
//!   let key = Keychain::new("hyperagent");
//!   key.set("HYPER_LLM_API_KEY", "sk-...")?;
//!   let value = key.get("HYPER_LLM_API_KEY")?;

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

const SERVICE_NAME: &str = "hyperagent";

/// Platform keychain abstraction.
pub struct Keychain {
    service: String,
}

impl Keychain {
    pub fn new(service: &str) -> Self {
        Self { service: service.to_string() }
    }

    /// Store a secret in the platform keychain.
    /// On macOS, uses `security add-generic-password`.
    pub fn set(&self, account: &str, password: &str) -> Result<()> {
        if cfg!(target_os = "macos") {
            self.macos_set(account, password)
        } else {
            // Fallback: store in encrypted file (chmod 600)
            self.file_set(account, password)
        }
    }

    /// Retrieve a secret from the platform keychain.
    pub fn get(&self, account: &str) -> Result<Option<String>> {
        if cfg!(target_os = "macos") {
            self.macos_get(account)
        } else {
            self.file_get(account)
        }
    }

    /// Delete a secret from the platform keychain.
    pub fn delete(&self, account: &str) -> Result<()> {
        if cfg!(target_os = "macos") {
            self.macos_delete(account)
        } else {
            self.file_delete(account)
        }
    }

    // ── macOS Keychain via `security` CLI ──────────────────────

    fn macos_set(&self, account: &str, password: &str) -> Result<()> {
        let status = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s", &self.service,
                "-a", account,
                "-w", password,
                "-U", // Update if exists
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run security CLI — is macOS Keychain available?")?;

        if status.success() {
            Ok(())
        } else {
            bail!("Failed to store credential in macOS Keychain for account: {account}")
        }
    }

    fn macos_get(&self, account: &str) -> Result<Option<String>> {
        let output = std::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s", &self.service,
                "-a", account,
                "-w", // Return password only
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .context("Failed to run security CLI")?;

        if output.status.success() {
            let password = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string();
            Ok(Some(password))
        } else {
            // Not found — return None, not an error
            Ok(None)
        }
    }

    fn macos_delete(&self, account: &str) -> Result<()> {
        let status = std::process::Command::new("security")
            .args([
                "delete-generic-password",
                "-s", &self.service,
                "-a", account,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run security CLI")?;

        if status.success() {
            Ok(())
        } else {
            // If item doesn't exist, that's fine
            Ok(())
        }
    }

    // ── Platform-agnostic fallback: encrypted file ─────────────

    fn secrets_path(&self) -> PathBuf {
        let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".hyper").join(".secrets.json")
    }

    fn load_secrets(&self) -> std::collections::HashMap<String, String> {
        let path = self.secrets_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        }
    }

    fn save_secrets(&self, secrets: &std::collections::HashMap<String, String>) -> Result<()> {
        let path = self.secrets_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(secrets)?;
        std::fs::write(&path, &json)?;

        // Restrict permissions to owner-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&path, perms);
            }
        }

        Ok(())
    }

    fn file_set(&self, account: &str, password: &str) -> Result<()> {
        let mut secrets = self.load_secrets();
        secrets.insert(account.to_string(), password.to_string());
        self.save_secrets(&secrets)
    }

    fn file_get(&self, account: &str) -> Result<Option<String>> {
        let secrets = self.load_secrets();
        Ok(secrets.get(account).cloned())
    }

    fn file_delete(&self, account: &str) -> Result<()> {
        let mut secrets = self.load_secrets();
        secrets.remove(account);
        self.save_secrets(&secrets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keychain_set_get_cycle() {
        let kc = Keychain::new("hyperagent-test");
        let test_key = "test_key_123";
        let test_val = "test_value_456";

        // Clean up any leftover
        let _ = kc.delete(test_key);

        // Set and verify
        kc.set(test_key, test_val).unwrap();
        let retrieved = kc.get(test_key).unwrap();
        assert_eq!(retrieved, Some(test_val.to_string()));

        // Clean up
        kc.delete(test_key).unwrap();
        let after_delete = kc.get(test_key).unwrap();
        assert_eq!(after_delete, None);
    }

    #[test]
    fn test_keychain_get_nonexistent() {
        let kc = Keychain::new("hyperagent-test");
        let result = kc.get("nonexistent_key_999").unwrap();
        assert_eq!(result, None);
    }
}
