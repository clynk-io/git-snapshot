use git_snapshot::repo_watcher::{RepoConfig, RepoWatcher, WatchConfig};
use git_snapshot::watcher::WatchMode;
use git_snapshot::Repo;
use serde_json::to_writer_pretty;
use std::env::current_dir;
use std::fs::OpenOptions;
use std::thread;
use std::time::Duration;
use structopt::StructOpt;

fn main() {
    let cwd = current_dir().unwrap();
    let repo = Repo::from_path(cwd).unwrap();
    repo.snapshot().unwrap();
    let f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("config.json")
        .unwrap();

    let watcher = RepoWatcher::new(WatchConfig {
        repos: vec![RepoConfig { path: "./".into() }],
        mode: WatchMode::Event,
        period: Duration::from_secs(30),
    })
    .unwrap();
    thread::park();
    drop(watcher)
}
