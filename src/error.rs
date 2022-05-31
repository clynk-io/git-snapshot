use thiserror::Error as ThisError;

#[derive(Debug, ThisError, PartialEq)]
pub enum Error {
    #[error("git error: {0:?}")]
    Git(#[from] git2::Error),
    #[error("invalid head")]
    InvalidHead,
}
