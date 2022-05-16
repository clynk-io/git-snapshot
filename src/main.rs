use git_snapshot::Repo;
use std::env::current_dir;

fn main() {
    let cwd = current_dir().unwrap();
    Repo::new(cwd, None).unwrap();
}
