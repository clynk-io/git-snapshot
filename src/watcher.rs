use crate::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::from_reader;
use std::{fs::OpenOptions, path::PathBuf};
use time::Duration;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", tag = "mode", content = "modeConfig")]
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
    #[serde(default, flatten)]
    pub mode: WatchMode,
}

pub struct Watcher {
    config_path: Option<PathBuf>,
    config: WatchConfig,
}

impl Watcher {
    pub fn from_config_path(config_path: impl Into<PathBuf>) -> Result<Self, Error> {
        let config_path = config_path.into();
        let f = OpenOptions::new().read(true).open(&config_path)?;
        let config: WatchConfig = from_reader(f)?;
        Ok(Watcher {
            config_path: Some(config_path),
            config: config,
        })
    }

    pub fn new(config: WatchConfig) -> Self {
        Watcher {
            config_path: None,
            config: config,
        }
    }

    pub fn watch() -> Result<(), Error> {
        todo!()
    }
}
