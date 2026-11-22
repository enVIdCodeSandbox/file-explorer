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

/// What a Windows or POSIX error number that comes from a share or a connection (not from the person's files) means.
pub fn connection_hint(code: i32) -> Option<&'static str> {
    // Windows: 53 path not found, 59 unexpected network error, 64 name no longer available, 67 no such share, 121 semaphore
    // timeout, 1117 device error, 1231 network location unreachable, 1236 connection aborted, 1450 / 1460 / 1359 timeouts.
    // POSIX (Linux, macOS): ETIMEDOUT, EHOSTUNREACH, ENETDOWN, ECONNRESET, ESTALE.
    let waiting = "The share stopped answering (the server, its disk or the connection), so the system gave up waiting.";
    if cfg!(windows) {
        match code {
            53 | 67 => Some("Windows could not find that computer or share."),
            // 32 sharing violation, 33 lock violation: the folder or file is open in another program, or on a share, on the
            // server (a backup job copying into it, say). Windows keeps retrying for minutes before giving up.
            32 | 33 => Some("Another program has it open or locked. On a network share that can be the server itself (a backup job running there, say). Close it or wait for it to finish, then try again."),
            59 | 64 | 121 | 1117 | 1231 | 1236 | 1359 | 1450 | 1460 => Some(waiting),
            _ => None,
        }
    } else {
        match code {
            110 | 113 | 100 | 104 | 116 => Some(waiting),
            _ => None,
        }
    }
}

impl CoreError {
    /// A plain-words addition to the message when the cause is the share or the connection rather than the app.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            CoreError::Io { source, .. } => source.raw_os_error().and_then(connection_hint),
            _ => None,
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_errors_get_a_plain_explanation() {
        let code = if cfg!(windows) { 121 } else { 110 };
        let e = CoreError::io("//srv/share/x", std::io::Error::from_raw_os_error(code));
        assert!(e.hint().unwrap().contains("stopped answering"));
        assert!(CoreError::io("x", std::io::Error::from_raw_os_error(if cfg!(windows) { 5 } else { 13 })).hint().is_none());
        if cfg!(windows) {
            assert!(CoreError::io("x", std::io::Error::from_raw_os_error(32)).hint().unwrap().contains("Another program"));
        }
        assert!(CoreError::Invalid("bad".into()).hint().is_none());
    }
}
