//! Sending alerts: Windows sound + notification, ntfy (phone), Discord.

use std::time::Duration;

use windows::core::HSTRING;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

use crate::capture;
use crate::settings::{data_dir, Alerts};

/// The name Windows knows the app's notifications by.
const APP_ID: &str = "PTSeedWatcher";

/// Phone and Discord alerts give up after this long.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);

/// Registers the app with Windows (for this user, no admin needed), so its
/// notifications show as "PT Seed Watcher" with the sprout icon, and players
/// can find it under Settings > System > Notifications.
pub fn register(icon_png: &[u8]) -> Result<(), String> {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let icon = dir.join("icon.png");
    if std::fs::read(&icon).ok().as_deref() != Some(icon_png) {
        let _ = std::fs::write(&icon, icon_png);
    }
    let key = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{APP_ID}"));
    for (name, value) in [("DisplayName", crate::app::APP_NAME.to_string()), ("IconUri", icon.display().to_string())] {
        let data: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &key,
                &HSTRING::from(name),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(format!("couldn't register for Windows notifications ({name}: {result:?})"));
        }
    }
    Ok(())
}

/// Sends the alert everywhere it's turned on. Returns a message for each
/// channel that failed; one broken channel doesn't stop the others.
pub fn send(alerts: &Alerts, title: &str, body: &str) -> Vec<String> {
    let mut failed = Vec::new();
    if alerts.sound {
        capture::beep();
    }
    if alerts.desktop {
        if let Err(e) = desktop(title, body) {
            failed.push(format!("Windows notification failed: {e}"));
        }
    }
    if !alerts.ntfy_topic.trim().is_empty() {
        if let Err(e) = ntfy(alerts, title, body) {
            failed.push(format!("Phone alert (ntfy) failed: {e}"));
        }
    }
    if !alerts.discord_webhook.trim().is_empty() {
        if let Err(e) = discord(alerts, title, body) {
            failed.push(format!("Discord alert failed: {e}"));
        }
    }
    failed
}

/// A Windows notification (toast).
pub fn desktop(title: &str, body: &str) -> Result<(), String> {
    notify_rust::Notification::new()
        .app_id(APP_ID)
        .summary(title)
        .body(body)
        .show()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn ntfy(alerts: &Alerts, title: &str, body: &str) -> Result<(), String> {
    let url = format!(
        "{}/{}",
        alerts.ntfy_server.trim().trim_end_matches('/'),
        alerts.ntfy_topic.trim()
    );
    ureq::post(&url)
        .timeout(NETWORK_TIMEOUT)
        .set("Title", title)
        .set("Priority", "high")
        .set("Tags", "seedling")
        .send_string(body)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn discord(alerts: &Alerts, title: &str, body: &str) -> Result<(), String> {
    let payload = serde_json::json!({ "content": format!("**{title}**\n{body}") }).to_string();
    ureq::post(alerts.discord_webhook.trim())
        .timeout(NETWORK_TIMEOUT)
        .set("Content-Type", "application/json")
        .send_string(&payload)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
