use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("git error: {0:?}")]
    Git(#[from] git2::Error),
    #[error("invalid head")]
    InvalidHead,
    #[error("other {0:?}")]
    Other(#[from] Box<dyn std::error::Error>),
}
