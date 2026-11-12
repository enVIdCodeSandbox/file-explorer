use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),
    #[error("not found: {0}")]
    NotFound(PathBuf),
    #[error("{0}")]
    Invalid(String),
    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        let path = path.into();
        match source.kind() {
            std::io::ErrorKind::PermissionDenied => CoreError::PermissionDenied(path),
            std::io::ErrorKind::NotFound => CoreError::NotFound(path),
            _ => CoreError::Io { path, source },
        }
    }

    /// True when retrying with elevated privileges could succeed.
    pub fn needs_elevation(&self) -> bool {
        matches!(self, CoreError::PermissionDenied(_))
    }
}

impl serde::Serialize for CoreError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
