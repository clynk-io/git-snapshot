use git_repository as git;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("git error {0:?}")]
    GitOpen(#[from] git::open::Error),
}
