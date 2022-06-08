use serde::{Deserialize, Serialize};
use serde_json::from_reader;
use std::{
    fs::{canonicalize, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant}, collections::HashMap,
};

use crate::{
    watcher::{WatchMode, Watcher},
    Error, Repo,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct WatchConfig {
    pub repos: Vec<RepoConfig>,
    pub mode: WatchMode,
    pub period: Duration,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoConfig {
    pub path: PathBuf,
}
pub struct RepoWatcher{
    watcher:  Arc<Mutex<Watcher>>,
    debounce_timestamps: Option<Arc<RwLock<HashMap<PathBuf, Instant>>>>
};

impl RepoWatcher {
    pub fn new(config: WatchConfig) -> Result<Self, Error> {
        let debounce_timestamps = match  config.mode {
           
        }
        Ok(Self{
            watcher: Arc::new(Mutex::new(Self::watcher(config)?)),
        })
    }

    fn open_config(config_path: &Path) -> Result<WatchConfig, Error> {
        let f = OpenOptions::new().read(true).open(config_path)?;
        Ok(from_reader(f)?)
    }

    pub fn with_config(config_path: impl AsRef<Path>) -> Result<Self, Error> {
        let config_path = config_path.as_ref();
        let config = Self::open_config(config_path)?;
        let watcher = Self::watcher(config)?;
        let watcher = Arc::new(Mutex::new(watcher));
        let watcher_clone = watcher.clone();
        watcher.lock().unwrap().watch_path(
            config_path,
            Box::new(move |path: PathBuf, handler_path: PathBuf| {
                let config = Self::open_config(&path);
                if let Ok(config) = config {
                    let w = Self::watcher(config).unwrap();
                    let mut watcher = watcher_clone.lock().unwrap();
                    *watcher = w;
                }
            }),
        )?;
        Ok(Self(watcher))
    }

    pub fn watcher(config: WatchConfig) -> Result<Watcher, Error> {
        let mut watcher = Watcher::new(&config.mode, Duration::from_millis(500))?;

        for RepoConfig { path } in &config.repos {
            let handler = move |path: PathBuf, handler_path: PathBuf| {
                let rel = path.strip_prefix(handler_path).unwrap();
                if rel.starts_with(".git") {
                    return;
                }
                if let Ok(repo) = Repo::from_path(&path) {
                    if !repo.is_ignored(rel).unwrap_or(false) {
                        repo.snapshot();
                        println!("Took snapshot")
                    }
                }
            };
            watcher.watch_path(canonicalize(path)?, Box::new(handler))?;
        }
        Ok(watcher)
    }
}
