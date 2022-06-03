use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("git error: {0:?}")]
    Git(#[from] git2::Error),
    #[error("invalid head")]
    InvalidHead,
    #[error("io error: {0:?}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0:?}")]
    Json(#[from] serde_json::error::Error),
}
