//! Pure SSH connections: file operations (using SFTP when offered, or shell commands over SSH)
//! and interactive PTY shell sessions rooted at the folder being browsed.

use super::sftp::{connect_handle, try_from_handle, Client};
use super::{fail, Control, Entry, Server, Session};
use crate::error::{CoreError, Result};
use russh::client::Handle;
use russh::ChannelMsg;
use std::io::{Read, Write};
use std::path::Path;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// Wrap `arg` in POSIX single quotes so paths with spaces, quotes or symbols survive remote `sh` execution.
pub fn shell_quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Connect over pure SSH: reuses the SFTP subsystem when available, falling back to POSIX shell
/// commands over `exec` channels when the remote server runs SSH without SFTP.
pub(super) fn connect(server: &Server, known_hosts: &Path) -> Result<Box<dyn Session>> {
    let (rt, handle) = connect_handle(server, known_hosts)?;
    match try_from_handle(rt, handle) {
        Ok(session) => Ok(session),
        Err((rt, handle, _)) => {
            let mut exec = SshExec { rt, handle };
            // Verify basic shell execution works before accepting the connection.
            let _ = exec.home()?;
            Ok(Box::new(exec))
        }
    }
}

struct SshExec {
    rt: Runtime,
    handle: Handle<Client>,
}

impl SshExec {
    fn exec_text(&mut self, command: &str) -> Result<(u32, String, String)> {
        let cmd = command.to_string();
        self.rt.block_on(async {
            let mut ch = self.handle.channel_open_session().await.map_err(fail)?;
            ch.exec(true, cmd.into_bytes()).await.map_err(fail)?;
            let (mut out, mut err, mut status) = (Vec::new(), Vec::new(), 0u32);
            while let Some(msg) = ch.wait().await {
                match msg {
                    ChannelMsg::Data { data } => out.extend_from_slice(&data),
                    ChannelMsg::ExtendedData { data, .. } => err.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status } => status = exit_status,
                    _ => {}
                }
            }
            Ok((
                status,
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).trim().to_string(),
            ))
        })
    }

    fn run_ok(&mut self, command: &str) -> Result<()> {
        let (code, _, err) = self.exec_text(command)?;
        if code == 0 {
            Ok(())
        } else {
            Err(fail(if err.is_empty() { format!("Remote command exited with status {code}.") } else { err }))
        }
    }
}

/// Parse tab-delimited entries written by our pure-SSH listing script (`D\tname` or `F\tname`).
pub(crate) fn parse_listing(stdout: &str) -> Vec<Entry> {
    stdout
        .lines()
        .filter_map(|line| {
            let (kind, rest) = line.split_once('\t')?;
            let (size, name) = match rest.split_once('\t') {
                Some((sz, n)) => (sz.parse::<u64>().unwrap_or(0), n),
                None => (0, rest),
            };
            if name.is_empty() || name == "." || name == ".." {
                return None;
            }
            let is_dir = kind == "D";
            Some(Entry { name: name.to_string(), is_dir, size: if is_dir { 0 } else { size }, modified: None })
        })
        .collect()
}

impl Session for SshExec {
    fn home(&mut self) -> Result<String> {
        let (code, out, err) = self.exec_text("pwd -P 2>/dev/null || pwd")?;
        if code != 0 && out.trim().is_empty() {
            return Err(fail(if err.is_empty() { "Could not open an SSH shell session." } else { &err }));
        }
        let dir = out.lines().next().unwrap_or("/").trim();
        Ok(if dir.starts_with('/') { dir.to_string() } else { format!("/{dir}") })
    }

    fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let q = shell_quote(path);
        // Builtin-only loop avoids column/locale differences across BusyBox, BSD and GNU `ls`.
        let cmd = format!(
            "cd {q} || exit 1; for f in .* *; do [ \"$f\" = \".\" ] || [ \"$f\" = \"..\" ] && continue; [ -e \"$f\" ] || [ -L \"$f\" ] || continue; if [ -d \"$f\" ]; then printf 'D\\t0\\t%s\\n' \"$f\"; else printf 'F\\t0\\t%s\\n' \"$f\"; fi; done"
        );
        let (code, out, err) = self.exec_text(&cmd)?;
        if code != 0 {
            return Err(fail(if err.is_empty() { format!("Could not open {path}") } else { err }));
        }
        Ok(parse_listing(&out))
    }

    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
        let mut sink = std::fs::File::create(local).map_err(|e| CoreError::io(local, e))?;
        let res = self.read_stream(
            remote,
            &mut |bytes| sink.write_all(bytes).map_err(|e| CoreError::io(local, e)),
            ctl,
        );
        if res.is_err() {
            let _ = std::fs::remove_file(local);
        }
        res
    }

    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
        let mut src = std::fs::File::open(local).map_err(|e| CoreError::io(local, e))?;
        let size = src.metadata().map(|m| m.len()).unwrap_or(0);
        self.write_stream(&mut src, size, remote, ctl)
    }

    fn read_head(&mut self, remote: &str, limit: usize) -> Result<Vec<u8>> {
        let cap = limit.min(64 * 1024);
        let q = shell_quote(remote);
        let cmd = format!("head -c {cap} {q} 2>/dev/null || dd bs=1 count={cap} < {q} 2>/dev/null");
        self.rt.block_on(async {
            let mut ch = self.handle.channel_open_session().await.map_err(fail)?;
            ch.exec(true, cmd.into_bytes()).await.map_err(fail)?;
            let mut out = Vec::new();
            while let Some(msg) = ch.wait().await {
                match msg {
                    ChannelMsg::Data { data } => {
                        let take = (cap - out.len()).min(data.len());
                        out.extend_from_slice(&data[..take]);
                        if out.len() >= cap {
                            break;
                        }
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }
            Ok(out)
        })
    }

    fn streams(&self) -> bool {
        true
    }

    fn read_stream(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
        let cmd = format!("cat {}", shell_quote(remote));
        self.rt.block_on(async {
            let mut ch = self.handle.channel_open_session().await.map_err(fail)?;
            ch.exec(true, cmd.into_bytes()).await.map_err(fail)?;
            let (mut total, mut status, mut err) = (0u64, 0u32, Vec::new());
            while let Some(msg) = ch.wait().await {
                ctl.check()?;
                match msg {
                    ChannelMsg::Data { data } => {
                        sink(&data)?;
                        total += data.len() as u64;
                    }
                    ChannelMsg::ExtendedData { data, .. } => err.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status } => status = exit_status,
                    _ => {}
                }
            }
            if status != 0 {
                let text = String::from_utf8_lossy(&err).trim().to_string();
                return Err(fail(if text.is_empty() { format!("Could not read {remote}") } else { text }));
            }
            Ok(total)
        })
    }

    fn write_stream(&mut self, source: &mut (dyn Read + Send), _size: u64, remote: &str, ctl: &Control) -> Result<u64> {
        let cmd = format!("cat > {}", shell_quote(remote));
        let res = self.rt.block_on(async {
            let ch = self.handle.channel_open_session().await.map_err(fail)?;
            ch.exec(true, cmd.into_bytes()).await.map_err(fail)?;
            let (mut read_half, write_half) = ch.split();
            let mut buf = vec![0u8; 64 * 1024];
            let mut total = 0u64;
            loop {
                ctl.check()?;
                let n = source.read(&mut buf).map_err(fail)?;
                if n == 0 {
                    break;
                }
                write_half.data(&buf[..n]).await.map_err(fail)?;
                total += n as u64;
                (ctl.on_bytes)(n as u64);
            }
            write_half.eof().await.map_err(fail)?;
            let (mut status, mut err) = (0u32, Vec::new());
            while let Some(msg) = read_half.wait().await {
                match msg {
                    ChannelMsg::ExtendedData { data, .. } => err.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status } => status = exit_status,
                    _ => {}
                }
            }
            if status != 0 {
                let text = String::from_utf8_lossy(&err).trim().to_string();
                return Err(fail(if text.is_empty() { format!("Could not write {remote}") } else { text }));
            }
            Ok(total)
        });
        if res.is_err() {
            let _ = self.exec_text(&format!("rm -f {}", shell_quote(remote)));
        }
        res
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.run_ok(&format!("mkdir -p {}", shell_quote(path)))
    }

    fn remove(&mut self, path: &str, is_dir: bool) -> Result<()> {
        let op = if is_dir { "rmdir" } else { "rm -f" };
        self.run_ok(&format!("{op} {}", shell_quote(path)))
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.run_ok(&format!("mv -f {} {}", shell_quote(from), shell_quote(to)))
    }

    fn exists(&mut self, path: &str) -> Result<bool> {
        let (code, _, _) = self.exec_text(&format!("test -e {}", shell_quote(path)))?;
        Ok(code == 0)
    }
}

// ---------------------------------------------------------------------------
// Interactive SSH PTY terminal session
// ---------------------------------------------------------------------------

enum TermCmd {
    Write(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

/// Handle to a running SSH interactive PTY shell.
pub struct SshTerminal {
    tx: UnboundedSender<TermCmd>,
}

impl SshTerminal {
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(TermCmd::Write(bytes));
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.tx.send(TermCmd::Resize { cols, rows });
    }

    pub fn close(&self) {
        let _ = self.tx.send(TermCmd::Close);
    }
}

/// Open an interactive PTY shell over SSH routed to `start_dir` (when non-empty).
pub fn open_terminal<FOut, FExit>(
    server: &Server,
    known_hosts: &Path,
    start_dir: &str,
    cols: u16,
    rows: u16,
    on_data: FOut,
    on_exit: FExit,
) -> Result<SshTerminal>
where
    FOut: Fn(&[u8]) + Send + 'static,
    FExit: FnOnce(Option<i32>) + Send + 'static,
{
    let (rt, handle) = connect_handle(server, known_hosts)?;
    let (tx, mut rx) = unbounded_channel::<TermCmd>();
    let target_dir = start_dir.trim().to_string();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
    std::thread::Builder::new()
        .name(format!("ssh-term-{}", server.host))
        .spawn(move || {
            let outcome = rt.block_on(async move {
                let channel = match handle.channel_open_session().await {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ready_tx.send(Err(fail(format!("Could not open SSH shell: {e}"))));
                        return;
                    }
                };
                if let Err(e) = channel
                    .request_pty(true, "xterm-256color", cols.max(2) as u32, rows.max(2) as u32, 0, 0, &[])
                    .await
                {
                    let _ = ready_tx.send(Err(fail(format!("Could not allocate SSH PTY: {e}"))));
                    return;
                }
                if let Err(e) = channel.request_shell(true).await {
                    let _ = ready_tx.send(Err(fail(format!("Could not start remote shell: {e}"))));
                    return;
                }
                let _ = ready_tx.send(Ok(()));

                let (mut read_half, write_half) = channel.split();
                let mut exit_code: Option<i32> = None;
                // Wait briefly for the remote prompt before routing to `target_dir`.
                let mut pending_cd = (!target_dir.is_empty()).then(|| format!(" cd {}\r", shell_quote(&target_dir)));
                let cd_deadline = tokio::time::sleep(std::time::Duration::from_millis(250));
                tokio::pin!(cd_deadline);

                loop {
                    tokio::select! {
                        biased;
                        cmd = rx.recv() => {
                            match cmd {
                                Some(TermCmd::Write(bytes)) => {
                                    let _ = write_half.data(&bytes[..]).await;
                                }
                                Some(TermCmd::Resize { cols, rows }) => {
                                    let _ = write_half.window_change(cols.max(2) as u32, rows.max(2) as u32, 0, 0).await;
                                }
                                Some(TermCmd::Close) | None => {
                                    let _ = write_half.eof().await;
                                    let _ = write_half.close().await;
                                    break;
                                }
                            }
                        }
                        () = &mut cd_deadline, if pending_cd.is_some() => {
                            if let Some(cd) = pending_cd.take() {
                                let _ = write_half.data(cd.as_bytes()).await;
                            }
                        }
                        msg = read_half.wait() => {
                            match msg {
                                Some(ChannelMsg::Data { data }) => {
                                    on_data(&data);
                                    if let Some(cd) = pending_cd.take() {
                                        let _ = write_half.data(cd.as_bytes()).await;
                                    }
                                }
                                Some(ChannelMsg::ExtendedData { data, .. }) => {
                                    on_data(&data);
                                }
                                Some(ChannelMsg::ExitStatus { exit_status }) => {
                                    exit_code = Some(exit_status as i32);
                                }
                                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                                _ => {}
                            }
                        }
                    }
                }
                on_exit(exit_code);
            });
            let _ = outcome;
        })
        .map_err(fail)?;

    ready_rx.recv().map_err(fail)??;
    Ok(SshTerminal { tx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths_with_spaces_and_single_quotes() {
        assert_eq!(shell_quote("/home/me/My Folder"), "'/home/me/My Folder'");
        assert_eq!(shell_quote("/tmp/it's here"), "'/tmp/it'\\''s here'");
    }

    #[test]
    fn parses_pure_ssh_tab_listing() {
        let entries = parse_listing("D\t0\t.\nD\t0\t..\nD\t0\tsub folder\nF\t128\tnotes.txt\n");
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].name.as_str(), entries[0].is_dir, entries[0].size), ("sub folder", true, 0));
        assert_eq!((entries[1].name.as_str(), entries[1].is_dir, entries[1].size), ("notes.txt", false, 128));
    }
}
