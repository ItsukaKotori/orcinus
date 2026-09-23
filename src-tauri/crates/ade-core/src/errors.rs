use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Not a valid git repository: {0}")]
    NotAGitRepository(String),
    #[error("Path is not allowed: {0}")]
    PathNotAllowed(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
