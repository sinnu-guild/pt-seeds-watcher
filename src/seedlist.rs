//! The shared list of seed names. Downloaded from GitHub at start-up so the
//! maintainer can add seeds without everyone downloading a new app. Falls
//! back to the last downloaded copy, then to the copy built into the app.

use serde::{Deserialize, Serialize};

use crate::settings::data_dir;

/// Where the list lives. Edit seeds.json in the repo and push to update everyone.
pub const URL: &str = "https://raw.githubusercontent.com/dthrcrpz/pt-seeds-watcher/main/seeds.json";

const BUILT_IN: &str = include_str!("../seeds.json");

#[derive(Clone, Serialize, Deserialize)]
pub struct SeedList {
    /// When the list was last changed, e.g. "2026-11-20".
    #[serde(default)]
    pub updated: String,
    /// Newest app version; older apps show an "update available" note.
    #[serde(default)]
    pub latest_app_version: String,
    /// Where to download the newest app.
    #[serde(default)]
    pub download_url: String,
    /// Optional note shown at the top of the app.
    #[serde(default)]
    pub message: String,
    pub seeds: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Online,
    Saved,
    BuiltIn,
}

fn parse(text: &str) -> Option<SeedList> {
    serde_json::from_str::<SeedList>(text)
        .ok()
        .filter(|l| !l.seeds.is_empty())
}

pub fn built_in() -> SeedList {
    parse(BUILT_IN).expect("built-in seeds.json is valid")
}

/// The copy saved by the last successful download.
pub fn saved() -> Option<SeedList> {
    parse(&std::fs::read_to_string(data_dir().join("seeds.json")).ok()?)
}

/// Downloads the list and saves a copy for when the player is offline.
pub fn download() -> Result<SeedList, String> {
    let text = ureq::get(URL)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("couldn't download the seed list: {e}"))?
        .into_string()
        .map_err(|e| format!("couldn't download the seed list: {e}"))?;
    let list = parse(&text).ok_or("the downloaded seed list is broken")?;
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("seeds.json"), &text);
    Ok(list)
}

/// True if version `a` (like "1.2.0") is newer than `b`.
pub fn is_newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u32> { v.trim().split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_list_loads() {
        assert!(built_in().seeds.contains(&"Baobab Tree Seed".to_string()));
    }

    #[test]
    fn version_compare() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("0.9", "1.0.0"));
    }
}
