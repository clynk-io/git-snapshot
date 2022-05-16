use crate::error::Error;
use git_repository as git;
use std::path::Path;
pub struct RepoConfig {
    path: String,
    snapshot_branch: String,
    remotes: Vec<Remote>,
    frequency: u64,
}

pub struct Repo {
    git_repo: git::Repository,
}

pub struct Remote {
    name: String,
    snapshot_branch: String,
}

impl Repo {
    pub fn new(path: impl AsRef<Path>, snapshot_branch: Option<String>) -> Result<Self, Error> {
        let git_repo = git::open(path.as_ref())?;
        println!("{:?}", git_repo.namespace());
        Ok(Self { git_repo })
    }
}
