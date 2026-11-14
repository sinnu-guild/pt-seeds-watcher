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
