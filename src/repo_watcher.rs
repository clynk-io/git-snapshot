use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::from_reader;
use std::{
    fs::{canonicalize, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    watcher::{WatchMode, Watcher},
    Error, Repo,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "camelCase")]
pub struct WatchConfig {
    pub repos: Vec<RepoConfig>,
    #[serde(flatten)]
    pub mode: WatchMode,
    #[serde(with = "humantime_serde")]
    pub debounce_period: Duration,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "camelCase")]
pub struct RepoConfig {
    pub path: PathBuf,
}

type SyncWatcher = Arc<Mutex<Watcher>>;

pub struct RepoWatcher(SyncWatcher);

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            repos: Vec::default(),
            mode: WatchMode::default(),
            debounce_period: Duration::from_secs(30),
        }
    }
}

impl Default for WatchMode {
    fn default() -> Self {
        WatchMode::Event
    }
}

impl RepoWatcher {
    pub fn new(config: WatchConfig) -> Result<Self, Error> {
        Ok(Self(Arc::new(Mutex::new(Self::watcher(config)?))))
    }

    fn open_config(config_path: &Path) -> Result<WatchConfig, Error> {
        let f = OpenOptions::new().read(true).open(config_path)?;
        Ok(from_reader(f)?)
    }

    pub fn with_config(config_path: impl AsRef<Path>) -> Result<Self, Error> {
        let config_path = config_path.as_ref();
        let config = Self::open_config(config_path)?;

        let debounce_period = config.debounce_period.clone();

        let watcher = Self::watcher(config)?;
        let watcher = Arc::new(Mutex::new(watcher));
        Self::watch_config(watcher.clone(), config_path, debounce_period)?;

        Ok(Self(watcher))
    }

    fn watcher(config: WatchConfig) -> Result<Watcher, Error> {
        let debounce_period = config.debounce_period.clone();
        let mut watcher = Watcher::new(&config.mode, debounce_period.clone())?;
        for RepoConfig { path } in &config.repos {
            let handler = move |path: PathBuf| {
                let rel = path.strip_prefix(&path).unwrap();
                if rel.starts_with(".git") {
                    return;
                }

                if let Ok(repo) = Repo::from_path(&path) {
                    if !repo.is_ignored(rel).unwrap_or(false) {
                        if let Err(err) = repo.snapshot() {
                            error!(target: repo.name(), "snapshot error: {:?}", err);
                        }
                    }
                }
            };
            watcher.watch_path(canonicalize(path)?, Box::new(handler))?;
        }
        Ok(watcher)
    }

    fn watch_config(
        watcher: SyncWatcher,
        config_path: &Path,
        period: Duration,
    ) -> Result<(), Error> {
        watcher.clone().lock().unwrap().watch_path(
            config_path,
            Box::new(move |path: PathBuf| {
                info!("Watcher detected config change, reloading config...");
                if let Ok(config) = Self::open_config(&path) {
                    if let Ok(w) = Self::watcher(config) {
                        let mut w_lock = watcher.lock().unwrap();
                        *w_lock = w;
                        drop(w_lock);
                        if let Err(err) = Self::watch_config(watcher.clone(), &path, period) {
                            error!("{:?}", err);
                        }
                    }
                }
            }),
        )
    }
}

impl WatchConfig {
    pub fn add_repo(&mut self, p: impl AsRef<Path>) -> Result<(), Error> {
        let p = canonicalize(p)?;
        if self.repos.iter().find(|&v| v.path == p).is_none() {
            self.repos.push(RepoConfig { path: p });
        }
        Ok(())
    }

    pub fn remove_repo(&mut self, p: impl AsRef<Path>) -> Result<(), Error> {
        let p = canonicalize(p)?;
        let index = self.repos.iter().position(|v| v.path == p);
        if let Some(index) = index {
            self.repos.remove(index);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use tempfile::{tempdir, NamedTempFile, TempDir};
    use tokio::time::sleep;

    use crate::{
        tests::check_snapshot_exists,
        util::tests::{create_temp_file, test_repo},
        watcher::WatchMode,
        Repo,
    };
    use serde_json::to_writer;

    fn test_repo_watcher(_mode: WatchMode) -> (TempDir, Repo, RepoWatcher) {
        let repo_path = tempdir().unwrap();
        let (repo, _) = test_repo(repo_path.path());
        let repo = Repo::new(repo);

        let repo_watcher = RepoWatcher::new(WatchConfig {
            repos: vec![RepoConfig {
                path: repo_path.path().to_owned(),
            }],
            mode: WatchMode::Event,
            debounce_period: Duration::from_millis(50),
        })
        .unwrap();

        (repo_path, repo, repo_watcher)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn repo_watcher() {
        let (repo_path, repo, repo_watcher) = test_repo_watcher(WatchMode::Event);
        create_temp_file(repo_path.path());

        sleep(Duration::from_millis(100)).await;
        drop(repo_watcher);

        assert!(check_snapshot_exists(&repo));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn config_file() {
        let repo_path = tempdir().unwrap();
        let (repo, _) = test_repo(repo_path.path());
        let repo = Repo::new(repo);
        let config_path = NamedTempFile::new().unwrap();
        let config = WatchConfig {
            repos: vec![RepoConfig {
                path: repo_path.path().to_owned(),
            }],
            mode: WatchMode::Event,
            debounce_period: Duration::from_millis(10),
        };
        to_writer(config_path.as_file(), &config).unwrap();

        let _repo_watcher = RepoWatcher::with_config(config_path.path()).unwrap();

        NamedTempFile::new_in(repo_path.path())
            .unwrap()
            .keep()
            .unwrap();
        sleep(Duration::from_millis(50)).await;
        assert!(check_snapshot_exists(&repo));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn config_file_change() {
        let repo_path1 = tempdir().unwrap();
        let (repo, _) = test_repo(repo_path1.path());
        let repo1 = Repo::new(repo);
        println!("Repo: {:?}", repo_path1);

        let repo_path2 = tempdir().unwrap();
        let (repo, _) = test_repo(repo_path2.path());
        let repo2 = Repo::new(repo);

        let config_path = NamedTempFile::new().unwrap();
        let config = WatchConfig {
            repos: vec![RepoConfig {
                path: repo_path1.path().to_owned(),
            }],
            mode: WatchMode::Event,
            debounce_period: Duration::from_millis(10),
        };
        to_writer(config_path.as_file(), &config).unwrap();

        let _repo_watcher = RepoWatcher::with_config(config_path.path()).unwrap();

        let config = WatchConfig {
            repos: vec![RepoConfig {
                path: repo_path2.path().to_owned(),
            }],
            mode: WatchMode::Event,
            debounce_period: Duration::from_millis(10),
        };
        to_writer(
            OpenOptions::new()
                .truncate(true)
                .write(true)
                .open(config_path.path())
                .unwrap(),
            &config,
        )
        .unwrap();

        sleep(Duration::from_millis(1000)).await;

        NamedTempFile::new_in(repo_path1.path())
            .unwrap()
            .keep()
            .unwrap();
        NamedTempFile::new_in(repo_path2.path())
            .unwrap()
            .keep()
            .unwrap();

        sleep(Duration::from_millis(50)).await;

        assert!(!check_snapshot_exists(&repo1));
        assert!(check_snapshot_exists(&repo2));
    }
}
