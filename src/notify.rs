//! Desktop notifications for agent events
//!
//! Supports:
//! - macOS: osascript (built-in) or terminal-notifier
//! - Linux: notify-send (libnotify)
//! - Windows: PowerShell toast notifications
//!
//! Fires on:
//! - Run complete (with summary)
//! - Approval needed
//! - Errors requiring attention

use anyhow::Result;

/// Notification event type
#[derive(Debug, Clone)]
pub enum NotificationEvent {
    /// Agent run completed successfully
    RunComplete {
        files_modified: usize,
        elapsed_secs: f64,
        tokens_used: usize,
    },
    /// Agent needs user approval
    ApprovalNeeded {
        description: String,
    },
    /// Error during run
    Error {
        message: String,
    },
    /// Generic info message
    Info {
        message: String,
    },
}

/// Send a desktop notification
pub fn notify(event: &NotificationEvent) -> Result<()> {
    let (title, message) = format_event(event);

    #[cfg(target_os = "macos")]
    {
        notify_macos(&title, &message)
    }
    #[cfg(target_os = "linux")]
    {
        notify_linux(&title, &message)
    }
    #[cfg(target_os = "windows")]
    {
        notify_windows(&title, &message)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (title, message);
        Ok(()) // Unsupported platform
    }
}

/// Format event into title and message
fn format_event(event: &NotificationEvent) -> (String, String) {
    match event {
        NotificationEvent::RunComplete { files_modified, elapsed_secs, tokens_used } => {
            (
                "✅ HyperAgent — Complete".to_string(),
                format!(
                    "Modified {} files in {:.1}s — ~{} tokens",
                    files_modified, elapsed_secs, tokens_used
                ),
            )
        }
        NotificationEvent::ApprovalNeeded { description } => {
            (
                "🤖 HyperAgent — Approval Needed".to_string(),
                description.clone(),
            )
        }
        NotificationEvent::Error { message } => {
            (
                "❌ HyperAgent — Error".to_string(),
                message.clone(),
            )
        }
        NotificationEvent::Info { message } => {
            (
                "ℹ️  HyperAgent".to_string(),
                message.clone(),
            )
        }
    }
}

/// macOS notification via osascript (built-in, no deps)
#[cfg(target_os = "macos")]
fn notify_macos(title: &str, message: &str) -> Result<()> {
    // Try terminal-notifier first (rich notifications)
    let has_terminal_notifier = std::process::Command::new("which")
        .arg("terminal-notifier")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_terminal_notifier {
        let output = std::process::Command::new("terminal-notifier")
            .args(["-title", title, "-message", message, "-sound", "default"])
            .output()?;
        if output.status.success() {
            return Ok(());
        }
    }

    // Fallback to osascript (always available on macOS)
    let script = format!(
        r#"display notification "{}" with title "{}" sound name "default""#,
        message.replace('"', "\\\""),
        title.replace('"', "\\\"")
    );

    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("User notification is not allowed") {
            // Only warn on real errors, not permission-denied
            eprintln!("   ⚠️  Notification failed: {}", stderr.trim());
        }
    }

    Ok(())
}

/// Linux notification via notify-send
#[cfg(target_os = "linux")]
fn notify_linux(title: &str, message: &str) -> Result<()> {
    let has_notify_send = std::process::Command::new("which")
        .arg("notify-send")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_notify_send {
        return Ok(()); // Silently skip if notify-send not available
    }

    let output = std::process::Command::new("notify-send")
        .args([title, message])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("   ⚠️  Notification failed: {}", stderr.trim());
    }

    Ok(())
}

/// Windows notification via PowerShell
#[cfg(target_os = "windows")]
fn notify_windows(title: &str, message: &str) -> Result<()> {
    let script = format!(
        r#"
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
$template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)
$textNodes = $template.GetElementsByTagName("text")
$textNodes.Item(0).AppendChild($template.CreateTextNode("{title}")) | Out-Null
$textNodes.Item(1).AppendChild($template.CreateTextNode("{message}")) | Out-Null
$toast = [Windows.UI.Notifications.ToastNotification]::new($template)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier().Show($toast)
"#,
        title = title.replace('"', "'"),
        message = message.replace('"', "'"),
    );

    let output = std::process::Command::new("powershell")
        .args(["-Command", &script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("   ⚠️  Notification failed: {}", stderr.trim());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_run_complete() {
        let event = NotificationEvent::RunComplete {
            files_modified: 3,
            elapsed_secs: 12.5,
            tokens_used: 4500,
        };
        let (title, msg) = format_event(&event);
        assert!(title.contains("Complete"));
        assert!(msg.contains("3 files"));
        assert!(msg.contains("12.5"));
        assert!(msg.contains("4500"));
    }

    #[test]
    fn test_format_approval() {
        let event = NotificationEvent::ApprovalNeeded {
            description: "Apply changes to src/main.rs".to_string(),
        };
        let (title, msg) = format_event(&event);
        assert!(title.contains("Approval"));
        assert!(msg.contains("src/main.rs"));
    }

    #[test]
    fn test_format_error() {
        let event = NotificationEvent::Error {
            message: "Network timeout".to_string(),
        };
        let (title, msg) = format_event(&event);
        assert!(title.contains("Error"));
        assert!(msg.contains("Network timeout"));
    }
}
