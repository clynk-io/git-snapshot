use git_snapshot::Repo;
use std::env::current_dir;
use structopt::StructOpt;
fn main() {
    let cwd = current_dir().unwrap();
    let repo = Repo::from_path(cwd).unwrap();
    repo.snapshot().unwrap();
}
