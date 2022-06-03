use git_snapshot::watcher::{WatchConfig, WatchMode};
use git_snapshot::Repo;
use serde_json::to_writer_pretty;
use std::env::current_dir;
use std::fs::OpenOptions;
use structopt::StructOpt;
use time::Duration;

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

    to_writer_pretty(
        f,
        &WatchConfig {
            mode: WatchMode::Event,
            repos: vec![],
        },
    )
    .unwrap();
}
