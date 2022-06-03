use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use time::Duration;

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
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
pub struct RepoConfig {
    path: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct WatchConfig {
    #[serde(default)]
    repos: Vec<RepoConfig>,
    #[serde(default)]
    mode: WatchMode,
}

pub struct Watcher {
    config_path: PathBuf,
}
