//! FTP. Plain FTP sends the password and the files unencrypted; the
//! interface says so when connecting.

use super::{fail, pump, Control, Entry, Server, Session};
use crate::error::{CoreError, Result};
use std::fs::File;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use suppaftp::types::FileType;
use suppaftp::FtpStream;

const TIMEOUT: Duration = Duration::from_secs(15);

struct Ftp {
    stream: FtpStream,
}

pub(super) fn connect(server: &Server) -> Result<Box<dyn Session>> {
    let addr = (server.host.as_str(), server.port)
        .to_socket_addrs()
        .map_err(|e| fail(format!("Could not find {}: {e}", server.host)))?
        .next()
        .ok_or_else(|| fail(format!("Could not find {}.", server.host)))?;
    let mut stream = FtpStream::connect_timeout(addr, TIMEOUT).map_err(|e| fail(format!("Could not connect to {}: {e}", server.host)))?;
    let user = if server.user.is_empty() { "anonymous" } else { server.user.as_str() };
    let password = if server.password.is_empty() && server.user.is_empty() { "anonymous@" } else { server.password.as_str() };
    stream.login(user, password).map_err(|e| fail(format!("The server did not accept the login: {e}")))?;
    stream.transfer_type(FileType::Binary).map_err(|e| fail(e))?;
    Ok(Box::new(Ftp { stream }))
}

fn entry(f: &suppaftp::list::File) -> Entry {
    let modified = f.modified().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64);
    Entry { name: f.name().to_string(), is_dir: f.is_directory(), size: if f.is_directory() { 0 } else { f.size() as u64 }, modified }
}

impl Session for Ftp {
    fn home(&mut self) -> Result<String> {
        self.stream.pwd().map_err(fail)
    }

    fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        // MLSD is exact and unambiguous; older servers only have LIST.
        let lines = match self.stream.mlsd(Some(path)) {
            Ok(lines) => lines.iter().filter_map(|l| suppaftp::list::ListParser::parse_mlsd(l).ok()).collect::<Vec<_>>(),
            Err(_) => self.stream.list(Some(path)).map_err(fail)?.iter().filter_map(|l| suppaftp::list::File::try_from(l.as_str()).ok()).collect(),
        };
        Ok(lines.iter().filter(|f| f.name() != "." && f.name() != "..").map(entry).collect())
    }

    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
        let mut out = File::create(local).map_err(|e| CoreError::io(local, e))?;
        let mut data = self.stream.retr_as_stream(remote).map_err(fail)?;
        let result = pump(&mut data, &mut out, ctl);
        match result {
            Ok(n) => {
                data.finish().map_err(fail)?;
                Ok(n)
            }
            Err(e) => {
                let _ = self.stream.abort(data);
                drop(out);
                let _ = std::fs::remove_file(local);
                Err(e)
            }
        }
    }

    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
        let mut src = File::open(local).map_err(|e| CoreError::io(local, e))?;
        let mut data = self.stream.put_with_stream(remote).map_err(fail)?;
        match pump(&mut src, &mut data, ctl) {
            Ok(n) => {
                data.finish().map_err(fail)?;
                Ok(n)
            }
            Err(e) => {
                let _ = self.stream.abort(data);
                let _ = self.stream.rm(remote);
                Err(e)
            }
        }
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.stream.mkdir(path).map_err(fail)
    }

    fn remove(&mut self, path: &str, is_dir: bool) -> Result<()> {
        if is_dir {
            self.stream.rmdir(path).map_err(fail)
        } else {
            self.stream.rm(path).map_err(fail)
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.stream.rename(from, to).map_err(fail)
    }
}

