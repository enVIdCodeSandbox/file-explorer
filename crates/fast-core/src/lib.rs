//! FAST File Explorer core engine.
//!
//! Everything in this crate is UI-agnostic: it knows nothing about Tauri or
//! the webview. The desktop/mobile shell (`src-tauri`) is a thin layer that
//! exposes these functions as commands. See `docs/ARCHITECTURE.md`.

pub mod details;
pub mod drives;
pub mod entry;
pub mod error;
pub mod history;
pub mod index;
pub mod ops;
pub mod platform;
pub mod predict;
pub mod resources;
pub mod search;
pub mod session;
pub mod shellmenu;
pub mod sysreport;
pub mod settings;
pub mod thumbs;
pub mod transfer;
pub mod treemap;
pub mod usage;

pub use error::{CoreError, Result};

/// Milliseconds since the Unix epoch for a `SystemTime`, if representable.
pub(crate) fn to_millis(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let t = t.ok()?;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_millis() as i64),
        Err(e) => Some(-(e.duration().as_millis() as i64)),
    }
}
