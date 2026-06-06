//! Cross-Platform Computer Use — Desktop automation for Windows & Linux.
//!
//! Extends the macOS-only computer_use to support all platforms.
//!
//! Platform backends:
//! - macOS: osascript + screencapture (existing in computer_use.rs)
//! - Linux: xdotool + scrot/import
//! - Windows: PowerShell + .NET Windows.Forms

use anyhow::{Context, Result, bail};

/// Unified computer use actions across all platforms
pub struct ComputerUse;

impl ComputerUse {
    /// Take a screenshot (platform-specific)
    pub fn screenshot(path: &str) -> Result<()> {
        if cfg!(target_os = "macos") {
            // Use native macOS screencapture tool (no crate dependency)
            let status = std::process::Command::new("screencapture")
                .args(["-x", "-C", path])
                .status()
                .context("Failed to take screenshot on macOS")?;
            if !status.success() {
                anyhow::bail!("screencapture failed");
            }
            Ok(())
        } else if cfg!(target_os = "linux") {
            // xdotool-based screenshot via scrot or import
            let result = std::process::Command::new("import")
                .args(["-window", "root", path])
                .status();
            if result.is_err() {
                // Try scrot as fallback
                std::process::Command::new("scrot")
                    .args([path])
                    .status()
                    .context("Neither 'import' (ImageMagick) nor 'scrot' found. Install one of them.")?;
            }
            Ok(())
        } else if cfg!(target_os = "windows") {
            // PowerShell screenshot using .NET
            let ps_script = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms
                Add-Type -AssemblyName System.Drawing
                $screen = [System.Windows.Forms.Screen]::PrimaryScreen
                $bitmap = New-Object System.Drawing.Bitmap $screen.Bounds.Width, $screen.Bounds.Height
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                $graphics.CopyFromScreen(0, 0, 0, 0, $bitmap.Size)
                $bitmap.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png)
                $graphics.Dispose()
                $bitmap.Dispose()
                "#,
                path
            );
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps_script])
                .status()
                .context("Failed to take screenshot on Windows")?;
            Ok(())
        } else {
            bail!("Unsupported platform for screenshots")
        }
    }

    /// Check if required tools are available
    pub fn check_available() -> Result<PlatformCapabilities> {
        let mut caps = PlatformCapabilities::default();
        caps.os = std::env::consts::OS.to_string();

        if cfg!(target_os = "macos") {
            caps.screenshot = std::process::Command::new("which")
                .arg("screencapture")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            caps.mouse_keyboard = std::process::Command::new("which")
                .arg("osascript")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
        } else if cfg!(target_os = "linux") {
            caps.screenshot = std::process::Command::new("which")
                .arg("import")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
                || std::process::Command::new("which")
                    .arg("scrot")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
            caps.mouse_keyboard = std::process::Command::new("which")
                .arg("xdotool")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
        } else if cfg!(target_os = "windows") {
            caps.screenshot = true; // PowerShell is always available
            caps.mouse_keyboard = true; // .NET is always available
        }

        Ok(caps)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlatformCapabilities {
    pub os: String,
    pub screenshot: bool,
    pub mouse_keyboard: bool,
}

impl PlatformCapabilities {
    pub fn print(&self) {
        println!();
        println!("  \x1b[1;36m🖥  Platform Capabilities\x1b[0m");
        println!("  {}", "─".repeat(40));
        println!("  OS:              {}", self.os);
        println!("  Screenshot:      {}", if self.screenshot { "✅" } else { "❌ (install ImageMagick or scrot)" });
        println!("  Mouse/Keyboard:  {}", if self.mouse_keyboard { "✅" } else { "❌ (install xdotool on Linux)" });
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_available() {
        let caps = ComputerUse::check_available();
        assert!(caps.is_ok());
    }
}
