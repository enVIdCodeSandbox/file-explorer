//! The built-in terminal: shells running in pseudo-terminals, kept by a small background
//! process (the *host*) so that they outlive the window, and the pieces that talk to it.
//!
//! Why a separate process: the terminal keeps what was printed and keeps programs running
//! when the app closes. A host that lives on its own can be reconnected to when the app starts
//! again, and a real terminal window (Terminal, Windows Terminal, GNOME Terminal…) can attach
//! to the very same shell, which is how "pop out when the app closes" keeps everything running.
//!
//! * [`proto`]  the messages (one JSON object per line over a loopback socket).
//! * [`host`]   the process that owns the sessions.
//! * [`client`] what the app and the pop-out window use to talk to it.
//! * [`popout`] opens a session in the system's own terminal app.
//! * [`attach`] the console program that shows one session in a real terminal window.

pub mod attach;
pub mod client;
pub mod host;
pub mod popout;
pub mod proto;

pub use client::Client;
pub use proto::{Event, Request, SessionInfo};
