#![allow(unused)]
//! Cross-Platform Computer Use — Desktop automation for Windows & Linux.
//!
//! Extends the macOS-only computer_use to support all platforms.
//!
//! Platform backends:
//! - macOS: osascript + screencapture (existing in computer_use.rs)
//! - Linux: xdotool + scrot/import
//! - Windows: PowerShell + .NET Windows.Forms

use anyhow::{Context, Result, bail};

/// Platform capability detection result
#[derive(Debug, Clone, Default)]
pub struct PlatformCapabilities {
    pub os: String,
    pub screenshot: bool,
    pub mouse_keyboard: bool,
}

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
        let mut caps = PlatformCapabilities {
            os: std::env::consts::OS.to_string(),
            ..Default::default()
        };

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

    /// Click at screen coordinates
    pub fn click(x: i32, y: i32) -> Result<()> {
        if cfg!(target_os = "macos") {
            let script = format!("tell application \"System Events\" to click at {{{}, {}}}", x, y);
            Self::run_osascript(&script)
        } else if cfg!(target_os = "linux") {
            std::process::Command::new("xdotool")
                .args(["mousemove", &x.to_string(), &y.to_string()])
                .status()?;
            std::process::Command::new("xdotool").args(["click", "1"]).status()?;
            Ok(())
        } else if cfg!(target_os = "windows") {
            let ps = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms
                   [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({x},{y})
                   Add-Type -MemberDefinition '[DllImport(\"user32.dll\")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, int dwExtraInfo);' -Name 'Mouse' -Namespace 'Win32';
                   [Win32.Mouse]::mouse_event(0x00000002, 0, 0, 0, 0);
                   [Win32.Mouse]::mouse_event(0x00000004, 0, 0, 0, 0);"#
            );
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .status()?;
            Ok(())
        } else {
            bail!("Unsupported platform for click")
        }
    }

    /// Type text at current cursor position
    pub fn type_text(text: &str) -> Result<()> {
        if cfg!(target_os = "macos") {
            let script = format!(
                "tell application \"System Events\" to keystroke \"{}\"",
                text.replace("\"", "\\\"")
            );
            Self::run_osascript(&script)
        } else if cfg!(target_os = "linux") {
            std::process::Command::new("xdotool")
                .args(["type", "--delay", "50", text])
                .status()?;
            Ok(())
        } else if cfg!(target_os = "windows") {
            let ps = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms
                   [System.Windows.Forms.SendKeys]::SendWait('{text}')"#,
                text = text.replace("'", "''")
            );
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .status()?;
            Ok(())
        } else {
            bail!("Unsupported platform for type_text")
        }
    }

    /// Press a key by name (e.g., "return", "escape", "tab")
    pub fn key_press(key: &str) -> Result<()> {
        if cfg!(target_os = "macos") {
            let script = format!("tell application \"System Events\" to key code {}", Self::mac_key_code(key)?);
            Self::run_osascript(&script)
        } else if cfg!(target_os = "linux") {
            std::process::Command::new("xdotool")
                .args(["key", key])
                .status()?;
            Ok(())
        } else if cfg!(target_os = "windows") {
            let key_map: std::collections::HashMap<&str, &str> = [
                ("return", "~"), ("escape", "{ESC}"), ("tab", "{TAB}"),
                ("up", "{UP}"), ("down", "{DOWN}"), ("left", "{LEFT}"), ("right", "{RIGHT}"),
            ].iter().cloned().collect();
            let mapped = key_map.get(key).map(|s| *s).unwrap_or(key);
            let ps = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms
                   [System.Windows.Forms.SendKeys]::SendWait('{key}')"#,
                key = mapped.replace("'", "''")
            );
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .status()?;
            Ok(())
        } else {
            bail!("Unsupported platform for key_press")
        }
    }

    /// Run an AppleScript on macOS (helper)
    #[cfg(target_os = "macos")]
    fn run_osascript(script: &str) -> Result<()> {
        std::process::Command::new("osascript")
            .args(["-e", script])
            .status()
            .context("osascript failed")?;
        Ok(())
    }

    /// Map key name to macOS key code
    #[cfg(target_os = "macos")]
    fn mac_key_code(key: &str) -> Result<u32> {
        match key {
            "a" => Ok(0), "s" => Ok(1), "d" => Ok(2), "f" => Ok(3),
            "h" => Ok(4), "g" => Ok(5), "z" => Ok(6), "x" => Ok(7),
            "c" => Ok(8), "v" => Ok(9), "b" => Ok(11), "q" => Ok(12),
            "w" => Ok(13), "e" => Ok(14), "r" => Ok(15), "y" => Ok(16),
            "t" => Ok(17), "1" => Ok(18), "2" => Ok(19), "3" => Ok(20),
            "4" => Ok(21), "6" => Ok(22), "5" => Ok(23), "=" => Ok(24),
            "9" => Ok(25), "7" => Ok(26), "-" => Ok(27), "8" => Ok(28),
            "0" => Ok(29), "]" => Ok(30), "o" => Ok(31), "u" => Ok(32),
            "[" => Ok(33), "i" => Ok(34), "p" => Ok(35), "l" => Ok(36),
            "j" => Ok(38), ";" => Ok(41), "k" => Ok(40), ";" => Ok(41),
            "\\" => Ok(42), "," => Ok(43), "/" => Ok(44), "n" => Ok(45),
            "m" => Ok(46), "." => Ok(47), "tab" => Ok(48), "space" => Ok(49),
            "`" => Ok(50), "return" => Ok(36), "enter" => Ok(36),
            "escape" => Ok(53), "delete" => Ok(51), "forwarddelete" => Ok(117),
            "up" => Ok(126), "down" => Ok(125), "left" => Ok(123), "right" => Ok(124),
            _ => bail!("Unknown macOS key: {}", key),
        }
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

    #[test]
    fn test_platform_capabilities_default() {
        let mut caps = PlatformCapabilities::default();
        caps.os = std::env::consts::OS.to_string();
        assert!(!caps.os.is_empty());
    }
}
