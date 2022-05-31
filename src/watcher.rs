use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoConfig {
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WatchConfig {}

pub struct Watcher {
    config_path: PathBuf,
}
