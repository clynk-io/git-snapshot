use git_snapshot::Repo;
use std::env::current_dir;

fn main() {
    let cwd = current_dir().unwrap();
    let repo = Repo::from_path(cwd).unwrap();
    repo.snapshot().unwrap();
}
