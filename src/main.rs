use git_snapshot::{Config, Remote, Repo};
use std::env::current_dir;

fn main() {
    let cwd = current_dir().unwrap();
    let repo = Repo::new(
        cwd,
        Config {
            remotes: vec![Remote {
                name: "gh".to_owned(),
            }],
        },
    )
    .unwrap();
    repo.snapshot().unwrap();
}
