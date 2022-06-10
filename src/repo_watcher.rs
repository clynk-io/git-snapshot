use log::error;
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
pub struct WatchConfig {
    pub repos: Vec<RepoConfig>,
    pub mode: WatchMode,
    pub debounce_period: Duration,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepoConfig {
    pub path: PathBuf,
}

type SyncWatcher = Arc<Mutex<Watcher>>;

pub struct RepoWatcher(SyncWatcher);

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

    pub fn watcher(config: WatchConfig) -> Result<Watcher, Error> {
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
                        if repo.snapshot().is_ok() {}
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

#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use tempfile::{tempdir, TempDir};

    use crate::{
        tests::check_snapshot_exists,
        util::tests::{create_temp_file, test_repo},
        watcher::WatchMode,
        Repo,
    };

    use super::{RepoConfig, RepoWatcher, WatchConfig};

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
    #[ignore]
    async fn event_watcher() {
        let (repo_path, repo, repo_watcher) = test_repo_watcher(WatchMode::Event);
        create_temp_file(repo_path.path());

        sleep(Duration::from_millis(100));
        drop(repo_watcher);

        assert!(check_snapshot_exists(&repo));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn event_watcher_debounce() {
        let (repo_path, repo, _repo_watcher) = test_repo_watcher(WatchMode::Event);
        create_temp_file(repo_path.path());
        sleep(Duration::from_millis(100));
        create_temp_file(repo_path.path());

        let snapshot_branch = Repo::snapshot_branch(
            &repo.git_repo().config().unwrap(),
            repo.current_branch().unwrap().as_str(),
        );

        let ref_log = repo
            .git_repo()
            .reflog(&format!("refs/heads/{}", snapshot_branch))
            .unwrap();

        assert_eq!(1, ref_log.len());

        sleep(Duration::from_millis(1000));

        let ref_log = repo
            .git_repo()
            .reflog(&format!("refs/heads/{}", snapshot_branch))
            .unwrap();

        assert_eq!(2, ref_log.len());
    }
}
