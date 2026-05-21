/// Errors that can occur in storage operations.
#[derive(Debug)]
pub enum StorageError {
    /// Codec serialization or deserialization failed.
    Codec(String),
    /// The requested object was not found in the store.
    NotFound,
    /// An underlying I/O error occurred.
    Io(std::io::Error),
    /// A Postgres client or server error occurred.
    Postgres(Box<tokio_postgres::Error>),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Codec(msg) => write!(f, "codec error: {msg}"),
            StorageError::NotFound => write!(f, "object not found"),
            StorageError::Io(e) => write!(f, "io error: {e}"),
            StorageError::Postgres(e) => write!(f, "postgres error: {e}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Io(e) => Some(e),
            StorageError::Postgres(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}

impl From<tokio_postgres::Error> for StorageError {
    fn from(e: tokio_postgres::Error) -> Self {
        StorageError::Postgres(Box::new(e))
    }
}

/// Convenience alias for `Result<T, StorageError>`.
pub type StorageResult<T> = Result<T, StorageError>;
