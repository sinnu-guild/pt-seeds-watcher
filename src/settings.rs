//! The player's choices, saved to %APPDATA%\pt-seeds-watcher\settings.json.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Seeds the player wants alerts for.
    pub picked: Vec<String>,
    /// Seeds the player typed in themselves (not in the shared list yet).
    pub custom_seeds: Vec<String>,
    /// Title of the game window. Empty = find "Play Together" automatically.
    pub window_title: String,
    /// Check right after each restock (every 5 minutes) instead of every
    /// `check_every_secs`.
    pub follow_restock: bool,
    /// How long after the restock to look, so the shop has time to update.
    pub restock_delay_secs: u64,
    /// Backup for PCs whose clock is off.
    pub check_every_secs: u64,
    pub alerts: Alerts,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            picked: Vec::new(),
            custom_seeds: Vec::new(),
            window_title: String::new(),
            follow_restock: true,
            restock_delay_secs: 7,
            check_every_secs: 20,
            alerts: Alerts::default(),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alerts {
    /// Windows notification.
    pub desktop: bool,
    /// Windows alert sound.
    pub sound: bool,
    /// ntfy server; the public one is fine.
    pub ntfy_server: String,
    /// ntfy topic to push to the player's phone. Empty = off.
    pub ntfy_topic: String,
    /// Discord webhook URL. Empty = off.
    pub discord_webhook: String,
    /// Alert once if the watcher can't see the player's seeds for a while.
    pub notify_when_window_lost: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Alerts {
            desktop: true,
            sound: true,
            ntfy_server: "https://ntfy.sh".into(),
            ntfy_topic: String::new(),
            discord_webhook: String::new(),
            notify_when_window_lost: true,
        }
    }
}

pub const DEFAULT_WINDOW: &str = "Play Together";

impl Settings {
    /// The game window title to look for.
    pub fn window(&self) -> &str {
        if self.window_title.trim().is_empty() {
            DEFAULT_WINDOW
        } else {
            &self.window_title
        }
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(data_dir().join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("settings.json"), text);
        }
    }
}

/// %APPDATA%\pt-seeds-watcher (falls back to the current folder).
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pt-seeds-watcher")
}
