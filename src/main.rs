use git_snapshot::repo_watcher::{RepoConfig, RepoWatcher, WatchConfig};
use git_snapshot::watcher::WatchMode;
use git_snapshot::Repo;

use std::env::current_dir;
use std::fs::OpenOptions;
use std::thread;
use std::time::Duration;

fn main() {
    let cwd = current_dir().unwrap();
    let repo = Repo::from_path(cwd).unwrap();
    repo.snapshot().unwrap();

    let _f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("config.json")
        .unwrap();

    let _watcher = RepoWatcher::new(WatchConfig {
        repos: vec![RepoConfig { path: "./".into() }],
        mode: WatchMode::Event,
        debounce_period: Duration::from_secs(10),
    })
    .unwrap();

    thread::park();
}
