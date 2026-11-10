//! Sending alerts: Windows sound + notification, ntfy (phone), Discord.

use crate::capture;
use crate::config::Alerts;

/// Sends the alert everywhere it's turned on. Failures are printed, not fatal,
/// so one broken channel doesn't stop the others or the watcher.
pub fn send(alerts: &Alerts, title: &str, body: &str) {
    if alerts.sound {
        capture::beep();
    }

    if alerts.desktop {
        if let Err(e) = notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .show()
        {
            eprintln!("    (Windows notification failed: {e})");
        }
    }

    if !alerts.ntfy_topic.trim().is_empty() {
        let url = format!(
            "{}/{}",
            alerts.ntfy_server.trim_end_matches('/'),
            alerts.ntfy_topic.trim()
        );
        let result = ureq::post(&url)
            .set("Title", title)
            .set("Priority", "high")
            .set("Tags", "seedling")
            .send_string(body);
        if let Err(e) = result {
            eprintln!("    (ntfy alert failed: {e})");
        }
    }

    if !alerts.discord_webhook.trim().is_empty() {
        let payload = serde_json::json!({ "content": format!("**{title}**\n{body}") }).to_string();
        let result = ureq::post(alerts.discord_webhook.trim())
            .set("Content-Type", "application/json")
            .send_string(&payload);
        if let Err(e) = result {
            eprintln!("    (Discord alert failed: {e})");
        }
    }
}
