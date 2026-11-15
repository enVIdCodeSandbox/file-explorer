//! Messages between the terminal host and its clients: one JSON object per line.
//! A client sends `{"id":1,"cmd":"list"}`; the host answers `{"re":1,"ok":true,"data":…}`.
//! Output and exits arrive unasked as `{"ev":"out","sid":"…","data":"<base64>"}`.

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
/// How much output each session keeps for people who attach later.
pub const RING_BYTES: usize = 2 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub sid: String,
    pub title: String,
    pub cwd: String,
    pub program: String,
    pub cols: u16,
    pub rows: u16,
    pub alive: bool,
    pub exit: Option<i32>,
    pub elevated: bool,
    /// Milliseconds since 1970.
    pub created: u64,
    /// Bytes of output so far (not just what is kept).
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Request {
    Hello { token: String },
    Spawn { cwd: String, program: Option<String>, args: Vec<String>, cols: u16, rows: u16, title: String, elevated: bool },
    List,
    /// Bytes typed (base64).
    Input { sid: String, data: String },
    Resize { sid: String, cols: u16, rows: u16 },
    Kill { sid: String },
    /// Forget a session that has ended.
    Remove { sid: String },
    /// Empty what is kept for the session (the shell itself is not touched).
    Clear { sid: String },
    /// What the session has printed so far; the connection then receives its new output.
    Snapshot { sid: String },
    Title { sid: String, title: String },
    /// Stop every session and the host.
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Frame {
    pub id: u64,
    #[serde(flatten)]
    pub req: Request,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Reply {
    pub re: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Event {
    /// "out" (new output), "exit" (the shell ended), "sessions" (one was added, removed or changed).
    pub ev: String,
    #[serde(default)]
    pub sid: String,
    /// Base64 output for "out".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

/// What is written in the host's info file so clients can find it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HostInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: u32,
}

/// Finds the folder a shell reports with the OSC 7 escape (`ESC ] 7 ; file://host/path BEL`),
/// which most shells can be set up to print after every command. `carry` holds the end of the
/// previous chunk, so a sequence split between two reads is still found. Returns the last one seen.
pub fn osc7_cwd(carry: &mut Vec<u8>, chunk: &[u8]) -> Option<String> {
    carry.extend_from_slice(chunk);
    let mut found = None;
    let mut from = 0;
    const START: &[u8] = b"\x1b]7;";
    while let Some(at) = carry[from..].windows(START.len()).position(|w| w == START).map(|p| p + from) {
        let body = at + START.len();
        let end = carry[body..].iter().position(|&b| b == 0x07 || b == 0x1b).map(|p| p + body);
        let Some(end) = end else { break };
        if let Ok(uri) = std::str::from_utf8(&carry[body..end]) {
            if let Some(rest) = uri.strip_prefix("file://") {
                // file://host/path: drop the host.
                let path = rest.find('/').map(|i| &rest[i..]).unwrap_or(rest);
                found = Some(percent_decode(path));
            }
        }
        from = end + 1;
    }
    // Keep only what may still be the start of a sequence.
    let keep_from = carry.iter().rposition(|&b| b == 0x1b).filter(|&p| p >= from).unwrap_or(carry.len());
    let tail = carry.split_off(keep_from);
    *carry = if tail.len() > 4096 { Vec::new() } else { tail };
    found
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    let text = String::from_utf8_lossy(&out).into_owned();
    // A Windows path arrives as /C:/Users/me.
    let bytes = text.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() {
        return text[1..].replace('/', "\\");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_with_their_id() {
        let line = serde_json::to_string(&Frame { id: 7, req: Request::Resize { sid: "a".into(), cols: 80, rows: 24 } }).unwrap();
        assert!(line.contains("\"id\":7") && line.contains("\"cmd\":\"resize\""));
        let back: Frame = serde_json::from_str(&line).unwrap();
        assert_eq!(back.id, 7);
        assert!(matches!(back.req, Request::Resize { cols: 80, rows: 24, .. }));
    }

    #[test]
    fn finds_the_folder_a_shell_reports() {
        let mut carry = Vec::new();
        assert_eq!(osc7_cwd(&mut carry, b"hello\x1b]7;file://box/home/me/My%20Docs\x07$ ").as_deref(), Some("/home/me/My Docs"));
        // Split across two reads.
        assert_eq!(osc7_cwd(&mut carry, b"x\x1b]7;file://box/tm"), None);
        assert_eq!(osc7_cwd(&mut carry, b"p/a\x1b\\").as_deref(), Some("/tmp/a"));
        // Windows drive paths.
        assert_eq!(osc7_cwd(&mut carry, b"\x1b]7;file:///C:/Users/me\x07").as_deref(), Some("C:\\Users\\me"));
    }

    #[test]
    fn ordinary_output_leaves_nothing_behind() {
        let mut carry = Vec::new();
        assert_eq!(osc7_cwd(&mut carry, b"plain text\r\n"), None);
        assert!(carry.is_empty());
    }
}
