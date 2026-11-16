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
    let mut stream = if super::proxy::current().is_some() {
        // Through the proxy: the control connection, and every data connection after it.
        let tcp = super::proxy::connect(&server.host, server.port, TIMEOUT).map_err(|e| fail(format!("Could not connect to {} through the proxy: {e}", server.host)))?;
        FtpStream::connect_with_stream(tcp)
            .map_err(|e| fail(format!("Could not connect to {}: {e}", server.host)))?
            .passive_stream_builder(|addr| super::proxy::connect_addr(addr, TIMEOUT).map_err(suppaftp::FtpError::ConnectionError))
    } else {
        let addr = (server.host.as_str(), server.port)
            .to_socket_addrs()
            .map_err(|e| fail(format!("Could not find {}: {e}", server.host)))?
            .next()
            .ok_or_else(|| fail(format!("Could not find {}.", server.host)))?;
        FtpStream::connect_timeout(addr, TIMEOUT).map_err(|e| fail(format!("Could not connect to {}: {e}", server.host)))?
    };
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

impl Ftp {
    /// A whole download with no resuming (the server refused to continue a part).
    fn download_plain(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
        let mut out = File::create(local).map_err(|e| CoreError::io(local, e))?;
        let mut data = self.stream.retr_as_stream(remote).map_err(fail)?;
        match pump(&mut data, &mut out, ctl) {
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
}

/// Where the unfinished part of `local` is kept while the server announced `size` bytes.
fn part_path(local: &Path, size: u64) -> std::path::PathBuf {
    let mut name = local.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{size}.fastpart"));
    local.with_file_name(name)
}

/// The part is complete: it becomes the file.
fn finish_part(part: &Path, local: &Path, total: u64) -> Result<u64> {
    let _ = std::fs::remove_file(local);
    std::fs::rename(part, local).map_err(|e| CoreError::io(local, e))?;
    Ok(total)
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
        // A download that was cut off (the connection dropped) leaves its first part next to the target, named for
        // the size the server announced; the next try asks the server to continue from there (REST).
        let size = self.stream.size(remote).ok().map(|s| s as u64);
        let part = size.map(|s| part_path(local, s));
        let have = part.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
        let resume = size.is_some_and(|s| have > 0 && have < s);
        if size.is_some_and(|s| have == s) {
            return finish_part(part.as_deref().unwrap(), local, have);
        }
        let target = part.as_deref().unwrap_or(local);
        let mut out = if resume {
            let f = std::fs::OpenOptions::new().append(true).open(target).map_err(|e| CoreError::io(target, e))?;
            self.stream.resume_transfer(have as usize).map_err(fail)?;
            f
        } else {
            File::create(target).map_err(|e| CoreError::io(target, e))?
        };
        let mut data = match self.stream.retr_as_stream(remote) {
            Ok(d) => d,
            Err(e) if resume => {
                // The server does not do REST: start over.
                let _ = e;
                drop(out);
                let _ = std::fs::remove_file(target);
                return self.download_plain(remote, local, ctl);
            }
            Err(e) => {
                drop(out);
                let _ = std::fs::remove_file(target);
                return Err(fail(e));
            }
        };
        if resume {
            (ctl.on_bytes)(have);
        }
        let result = pump(&mut data, &mut out, ctl);
        match result {
            Ok(n) => {
                data.finish().map_err(fail)?;
                drop(out);
                match part {
                    Some(p) => finish_part(&p, local, have + n),
                    None => Ok(n),
                }
            }
            Err(e) => {
                let _ = self.stream.abort(data);
                drop(out);
                // Stopped on purpose: nothing is kept. Cut off by the connection: the part stays, to be continued.
                if ctl.check().is_err() || part.is_none() {
                    let _ = std::fs::remove_file(target);
                }
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{Protocol, Server};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// Only one test at a time may set the app-wide proxy.
    static PROXY_LOCK: Mutex<()> = Mutex::new(());

    fn data() -> Vec<u8> {
        (0..100_000u32).map(|i| (i % 251) as u8).collect()
    }

    /// A very small FTP server: login, SIZE, REST, PASV and RETR of one file called `file.bin`. Returns its port and
    /// the offsets it was asked to continue from.
    fn fake_ftp() -> (u16, Arc<Mutex<Vec<usize>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let rests = Arc::new(Mutex::new(Vec::new()));
        let seen = rests.clone();
        thread::spawn(move || {
            for control in listener.incoming().flatten() {
                let rests = seen.clone();
                thread::spawn(move || {
                    let mut w = control.try_clone().unwrap();
                    let mut r = BufReader::new(control);
                    w.write_all(b"220 ready\r\n").unwrap();
                    let mut rest = 0usize;
                    let mut pasv: Option<TcpListener> = None;
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        let (cmd, arg) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
                        match cmd.to_ascii_uppercase().as_str() {
                            "USER" => w.write_all(b"331 password please\r\n").unwrap(),
                            "PASS" => w.write_all(b"230 welcome\r\n").unwrap(),
                            "TYPE" => w.write_all(b"200 ok\r\n").unwrap(),
                            "SIZE" => w.write_all(format!("213 {}\r\n", data().len()).as_bytes()).unwrap(),
                            "REST" => {
                                rest = arg.parse().unwrap();
                                rests.lock().unwrap().push(rest);
                                w.write_all(b"350 restarting\r\n").unwrap();
                            }
                            "PASV" => {
                                let l = TcpListener::bind("127.0.0.1:0").unwrap();
                                let p = l.local_addr().unwrap().port();
                                w.write_all(format!("227 Entering Passive Mode (127,0,0,1,{},{})\r\n", p >> 8, p & 255).as_bytes()).unwrap();
                                pasv = Some(l);
                            }
                            "RETR" => {
                                w.write_all(b"150 here it comes\r\n").unwrap();
                                let (mut d, _) = pasv.take().unwrap().accept().unwrap();
                                d.write_all(&data()[rest..]).unwrap();
                                drop(d);
                                rest = 0;
                                w.write_all(b"226 done\r\n").unwrap();
                            }
                            "QUIT" => {
                                w.write_all(b"221 bye\r\n").unwrap();
                                break;
                            }
                            _ => w.write_all(b"502 not here\r\n").unwrap(),
                        }
                    }
                });
            }
        });
        (port, rests)
    }

    /// A SOCKS5 proxy that needs no login and relays; counts the connections it carried.
    fn fake_socks() -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        thread::spawn(move || {
            for mut c in listener.incoming().flatten() {
                let seen = seen.clone();
                thread::spawn(move || {
                    let mut g = [0u8; 3];
                    c.read_exact(&mut g).unwrap();
                    c.write_all(&[5, 0]).unwrap();
                    let mut head = [0u8; 4];
                    c.read_exact(&mut head).unwrap();
                    let target: std::net::SocketAddr = match head[3] {
                        1 => {
                            let mut b = [0u8; 6];
                            c.read_exact(&mut b).unwrap();
                            (std::net::Ipv4Addr::new(b[0], b[1], b[2], b[3]), u16::from_be_bytes([b[4], b[5]])).into()
                        }
                        _ => {
                            let mut l = [0u8; 1];
                            c.read_exact(&mut l).unwrap();
                            let mut name = vec![0u8; l[0] as usize + 2];
                            c.read_exact(&mut name).unwrap();
                            let port = u16::from_be_bytes([name[name.len() - 2], name[name.len() - 1]]);
                            ([127, 0, 0, 1], port).into()
                        }
                    };
                    let up = TcpStream::connect(target).unwrap();
                    seen.fetch_add(1, Ordering::SeqCst);
                    c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
                    let (mut a, b) = (c.try_clone().unwrap(), up.try_clone().unwrap());
                    let t = thread::spawn(move || {
                        let _ = std::io::copy(&mut a, &mut &b);
                        let _ = b.shutdown(std::net::Shutdown::Write);
                    });
                    let (mut up, mut c) = (up, c);
                    let _ = std::io::copy(&mut up, &mut c);
                    // The server is done sending: tell the client too, or it waits for more.
                    let _ = c.shutdown(std::net::Shutdown::Write);
                    let _ = t.join();
                });
            }
        });
        (port, count)
    }

    fn server(port: u16) -> Server {
        Server { protocol: Protocol::Ftp, host: "127.0.0.1".into(), port, user: "u".into(), password: "p".into(), key_file: None, tls: false, trust: None }
    }

    fn fetch(port: u16, dir: &Path) -> Vec<u8> {
        let mut s = connect(&server(port)).unwrap();
        let cancel = AtomicBool::new(false);
        let moved = |_: u64| {};
        let ctl = Control { cancel: &cancel, on_bytes: &moved };
        let local = dir.join("file.bin");
        s.download("file.bin", &local, &ctl).unwrap();
        std::fs::read(&local).unwrap()
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fast-ftp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn part_names_carry_the_announced_size() {
        let p = part_path(Path::new("/tmp/a/movie.mkv"), 1234);
        assert_eq!(p, Path::new("/tmp/a/movie.mkv.1234.fastpart"));
    }

    #[test]
    fn a_finished_part_replaces_the_file() {
        let dir = temp("finish");
        let local = dir.join("f.bin");
        let part = part_path(&local, 3);
        std::fs::write(&local, b"old old old").unwrap();
        std::fs::write(&part, b"new").unwrap();
        assert_eq!(finish_part(&part, &local, 3).unwrap(), 3);
        assert_eq!(std::fs::read(&local).unwrap(), b"new");
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn downloads_a_whole_file() {
        let _g = PROXY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::remote::proxy::set("").unwrap();
        let (port, rests) = fake_ftp();
        let dir = temp("whole");
        assert_eq!(fetch(port, &dir), data());
        assert!(rests.lock().unwrap().is_empty());
        assert!(!dir.join(format!("file.bin.{}.fastpart", data().len())).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn continues_a_cut_off_download_where_it_stopped() {
        let _g = PROXY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::remote::proxy::set("").unwrap();
        let (port, rests) = fake_ftp();
        let dir = temp("resume");
        std::fs::write(dir.join(format!("file.bin.{}.fastpart", data().len())), &data()[..40_000]).unwrap();
        assert_eq!(fetch(port, &dir), data());
        assert_eq!(*rests.lock().unwrap(), vec![40_000]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn control_and_data_connections_go_through_the_proxy() {
        let _g = PROXY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (port, _) = fake_ftp();
        let (proxy_port, carried) = fake_socks();
        crate::remote::proxy::set(&format!("socks5://127.0.0.1:{proxy_port}")).unwrap();
        let dir = temp("proxy");
        let got = fetch(port, &dir);
        crate::remote::proxy::set("").unwrap();
        assert_eq!(got, data());
        assert_eq!(carried.load(Ordering::SeqCst), 2, "one control and one data connection");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
