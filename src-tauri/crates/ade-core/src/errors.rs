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
    #[error("git {command} failed (exit code {exit_code:?}): {stderr}")]
    GitCommandFailed {
        command: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    #[error("git {command} was cancelled")]
    GitCommandCancelled { command: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_command_failed_display_includes_command_stderr_and_code() {
        let error = CoreError::GitCommandFailed {
            command: "commit -m x".to_string(),
            stderr: "hook declined".to_string(),
            exit_code: Some(1),
        };
        assert_eq!(
            error.to_string(),
            "git commit -m x failed (exit code Some(1)): hook declined"
        );
    }

    #[test]
    fn git_command_cancelled_display() {
        let error = CoreError::GitCommandCancelled {
            command: "status --porcelain=v2".to_string(),
        };
        assert_eq!(error.to_string(), "git status --porcelain=v2 was cancelled");
    }
}
