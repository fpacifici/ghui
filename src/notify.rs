//! Native desktop notifications; see spec 0004.
//!
//! On macOS they go through `osascript`: `notify-rust` posts them on behalf
//! of Finder through the deprecated `NSUserNotificationCenter`, which
//! recent macOS versions drop silently. Elsewhere `notify-rust` talks to
//! the D-Bus notification service.

/// Shows a notification. Blocks until it is handed to the OS, so call it
/// off the UI thread.
#[cfg(target_os = "macos")]
pub fn send(title: &str, body: &str) -> Result<(), String> {
    let output = std::process::Command::new("osascript")
        .args(osascript_args(title, body))
        .output()
        .map_err(|e| format!("osascript: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("osascript: {}", stderr.trim()))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn send(title: &str, body: &str) -> Result<(), String> {
    notify_rust::Notification::new()
        .appname("ghui")
        .summary(title)
        .body(body)
        .show()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Passes the texts as arguments rather than in the script, so they need no
/// AppleScript escaping.
#[cfg(any(target_os = "macos", test))]
fn osascript_args<'a>(title: &'a str, body: &'a str) -> [&'a str; 8] {
    [
        "-e",
        "on run argv",
        "-e",
        "display notification (item 2 of argv) with title (item 1 of argv)",
        "-e",
        "end run",
        title,
        body,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_arguments_not_script() {
        let args = osascript_args("Checks \"passed\"", "o/a #1 it's done");
        assert_eq!(args[6], "Checks \"passed\"");
        assert_eq!(args[7], "o/a #1 it's done");
        assert!(args[..6].iter().all(|a| !a.contains("passed")));
    }
}
