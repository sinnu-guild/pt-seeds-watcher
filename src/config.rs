//! Settings read from config.toml.

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Title of the game window. Case and spaces are ignored.
    pub window_title: String,
    /// Seconds between checks.
    pub check_every_secs: u64,
    /// How close a match must be (0-1) to count as "in stock".
    pub threshold: f32,
    /// After an alert, the seed must be missing this many checks in a row
    /// before it can alert again. Stops repeat alerts for the same stock.
    pub rearm_after_misses: u32,
    /// Optional [x, y, width, height] inside the game window to search.
    /// Pixel positions come from snapshot.png.
    pub region: Option<[u32; 4]>,
    /// Folder (next to config.toml) holding the seed pictures.
    pub templates_dir: String,
    pub alerts: Alerts,
    pub seeds: Vec<Seed>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window_title: "Play Together".into(),
            check_every_secs: 20,
            threshold: 0.85,
            rearm_after_misses: 3,
            region: None,
            templates_dir: "templates".into(),
            alerts: Alerts::default(),
            seeds: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Alerts {
    /// Windows notification.
    pub desktop: bool,
    /// Windows alert sound.
    pub sound: bool,
    /// ntfy server; the public one is fine.
    pub ntfy_server: String,
    /// ntfy topic to push to your phone. Empty = off.
    pub ntfy_topic: String,
    /// Discord webhook URL. Empty = off.
    pub discord_webhook: String,
    /// Alert once if the watcher can't see the seed shop for a few checks in a row.
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    pub name: String,
    /// Picture file in templates_dir. Defaults to "<name>.png".
    #[serde(default)]
    pub template: Option<String>,
    /// Overrides the global threshold for this seed.
    #[serde(default)]
    pub threshold: Option<f32>,
}
