use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use time::Duration;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchMode {
    Poll { period: Duration },
    Event,
}

impl Default for WatchMode {
    fn default() -> Self {
        Self::Event
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoConfig {
    pub path: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchConfig {
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub mode: WatchMode,
}

pub struct Watcher {
    config_path: PathBuf,
}
