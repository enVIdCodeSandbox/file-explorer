//! Runs the same checks against real FTP, SFTP and WebDAV servers.
//!
//! The servers are started by `scripts/test-servers.sh`; without it these
//! tests are skipped. Set FAST_TEST_SERVERS=1 to turn the skip into a failure.

use fast_core::remote::{self, Control, Protocol, Server};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

fn up(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), std::time::Duration::from_millis(300)).is_ok()
}

fn server(protocol: Protocol, port: u16) -> Server {
    Server { protocol, host: "127.0.0.1".into(), port, user: "tester".into(), password: "secret".into(), key_file: None, tls: false, trust: None }
}

fn ctl_run<T>(f: impl FnOnce(&Control) -> T) -> (T, u64) {
    let cancel = AtomicBool::new(false);
    let moved = AtomicU64::new(0);
    let on = |n: u64| {
        moved.fetch_add(n, Ordering::Relaxed);
    };
    let ctl = Control { cancel: &cancel, on_bytes: &on };
    let r = f(&ctl);
    (r, moved.load(Ordering::Relaxed))
}

fn sample(dir: &Path) {
    fs::create_dir_all(dir.join("sub/deeper")).unwrap();
    fs::write(dir.join("a.txt"), "hello").unwrap();
    fs::write(dir.join("sp ace & 100%.bin"), vec![7u8; 300_000]).unwrap();
    fs::write(dir.join("sub/b.txt"), "bee").unwrap();
    fs::write(dir.join("sub/deeper/ü.txt"), "umlaut").unwrap();
    fs::write(dir.join("empty"), "").unwrap();
}

fn same(a: &Path, b: &Path) {
    for e in fs::read_dir(a).unwrap() {
        let e = e.unwrap();
        let other = b.join(e.file_name());
        if e.path().is_dir() {
            same(&e.path(), &other);
        } else {
            assert_eq!(fs::read(e.path()).unwrap(), fs::read(&other).unwrap(), "{:?}", other);
        }
    }
}

fn exercise(name: &str, s: Server, hosts: &Path) {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = remote::connect(&s, hosts).unwrap_or_else(|e| panic!("{name}: connect: {e}"));
    let home = session.home().unwrap();
    assert!(home.starts_with('/'), "{name}: home {home}");
    let base = format!("{}/t-{}", home.trim_end_matches('/'), std::process::id());
    session.mkdir(&base).unwrap_or_else(|e| panic!("{name}: mkdir: {e}"));
    assert!(session.exists(&base).unwrap(), "{name}: exists");
    assert!(!session.exists(&format!("{base}/nope")).unwrap(), "{name}: not exists");

    // Upload a tree, list it, download it back, compare.
    let src = tmp.path().join("src");
    sample(&src);
    let mut tally = remote::Tally::default();
    let (r, moved) = ctl_run(|c| remote::upload_tree(&mut *session, &src, &base, true, c, &mut tally));
    r.unwrap_or_else(|e| panic!("{name}: upload: {e}"));
    assert!(tally.failed.is_empty(), "{name}: {:?}", tally.failed);
    assert_eq!(tally.files, 5, "{name}");
    assert_eq!(moved, 300_000 + 5 + 3 + 6, "{name}: bytes");

    let root = format!("{base}/src");
    let mut names: Vec<_> = session.list(&root).unwrap().into_iter().map(|e| (e.name, e.is_dir, e.size)).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            ("a.txt".into(), false, 5),
            ("empty".into(), false, 0),
            ("sp ace & 100%.bin".into(), false, 300_000),
            ("sub".into(), true, 0),
        ],
        "{name}"
    );
    let deeper = session.list(&format!("{root}/sub/deeper")).unwrap();
    assert_eq!(deeper.len(), 1, "{name}");
    assert_eq!(deeper[0].name, "ü.txt", "{name}");
    assert!(deeper[0].modified.is_some(), "{name}: modified time");

    let back = tmp.path().join("back");
    fs::create_dir_all(&back).unwrap();
    let mut tally = remote::Tally::default();
    let (r, _) = ctl_run(|c| remote::download_tree(&mut *session, &root, true, &back, true, c, &mut tally));
    r.unwrap_or_else(|e| panic!("{name}: download: {e}"));
    assert!(tally.failed.is_empty(), "{name}: {:?}", tally.failed);
    same(&src, &back.join("src"));
    same(&back.join("src"), &src);

    // Not replacing keeps what is there.
    fs::write(back.join("src/a.txt"), "changed").unwrap();
    let mut tally = remote::Tally::default();
    ctl_run(|c| remote::download_tree(&mut *session, &root, true, &back, false, c, &mut tally)).0.unwrap();
    assert_eq!(fs::read_to_string(back.join("src/a.txt")).unwrap(), "changed", "{name}");

    // Rename a file and a folder.
    session.rename(&format!("{root}/a.txt"), &format!("{root}/renamed.txt")).unwrap_or_else(|e| panic!("{name}: rename file: {e}"));
    session.rename(&format!("{root}/sub"), &format!("{root}/sub2")).unwrap_or_else(|e| panic!("{name}: rename dir: {e}"));
    assert!(session.exists(&format!("{root}/renamed.txt")).unwrap(), "{name}");
    assert!(!session.exists(&format!("{root}/a.txt")).unwrap(), "{name}");
    assert!(session.exists(&format!("{root}/sub2/b.txt")).unwrap(), "{name}");

    // Cancel part-way: no half file is left locally or on the server.
    let big = tmp.path().join("big.bin");
    fs::write(&big, vec![1u8; 20_000_000]).unwrap();
    let cancel = AtomicBool::new(false);
    let seen = AtomicU64::new(0);
    let on = |n: u64| {
        if seen.fetch_add(n, Ordering::Relaxed) > 1_000_000 {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let ctl = Control { cancel: &cancel, on_bytes: &on };
    assert!(session.upload(&big, &format!("{base}/big.bin"), &ctl).is_err(), "{name}: cancelled upload");
    assert!(!session.exists(&format!("{base}/big.bin")).unwrap(), "{name}: half upload removed");
    // The connection still works afterwards.
    assert!(session.list(&base).is_ok(), "{name}: usable after cancel");
    ctl_run(|c| session.upload(&big, &format!("{base}/big.bin"), c)).0.unwrap_or_else(|e| panic!("{name}: big upload: {e}"));
    let cancel2 = AtomicBool::new(false);
    let seen2 = AtomicU64::new(0);
    let on2 = |n: u64| {
        if seen2.fetch_add(n, Ordering::Relaxed) > 1_000_000 {
            cancel2.store(true, Ordering::Relaxed);
        }
    };
    let ctl2 = Control { cancel: &cancel2, on_bytes: &on2 };
    let out = tmp.path().join("big.out");
    assert!(session.download(&format!("{base}/big.bin"), &out, &ctl2).is_err(), "{name}: cancelled download");
    assert!(!out.exists(), "{name}: half download removed");
    assert!(session.list(&base).is_ok(), "{name}: usable after cancelled download");

    // Errors are readable, not panics.
    assert!(session.list(&format!("{base}/missing")).is_err(), "{name}: list missing");
    assert!(ctl_run(|c| session.download(&format!("{base}/missing.txt"), &tmp.path().join("m"), c)).0.is_err(), "{name}: download missing");

    // Delete the lot.
    let (n, _) = ctl_run(|c| remote::remove_tree(&mut *session, &base, true, c));
    n.unwrap_or_else(|e| panic!("{name}: remove: {e}"));
    assert!(!session.exists(&base).unwrap(), "{name}: removed");
}

fn run(name: &str, protocol: Protocol, port: u16) {
    if !up(port) {
        assert!(std::env::var("FAST_TEST_SERVERS").is_err(), "{name} server is not running");
        eprintln!("skipping {name}: no server on port {port}");
        return;
    }
    exercise(name, server(protocol, port), Path::new("/nonexistent/known_hosts"));
}

#[test]
fn ftp() {
    run("ftp", Protocol::Ftp, 2121);
}

#[test]
fn webdav() {
    run("webdav", Protocol::Dav, 8081);
}

#[test]
fn sftp() {
    if !up(2222) {
        assert!(std::env::var("FAST_TEST_SERVERS").is_err(), "sftp server is not running");
        eprintln!("skipping sftp: no server on port 2222");
        return;
    }
    // A host seen for the first time must be confirmed, then it is remembered.
    let tmp = tempfile::tempdir().unwrap();
    let hosts = tmp.path().join("known_hosts");
    let s = server(Protocol::Sftp, 2222);
    let err = remote::connect(&s, &hosts).err().expect("unknown host must not connect").to_string();
    let fp = err.strip_prefix(remote::UNTRUSTED_HOST).unwrap_or_else(|| panic!("got: {err}")).to_string();
    assert!(fp.starts_with("SHA256:"));
    let mut trusted = s.clone();
    trusted.trust = Some(fp.clone());
    remote::connect(&trusted, &hosts).unwrap();
    // Remembered: no confirmation needed any more.
    remote::connect(&s, &hosts).unwrap();
    // A different key is refused.
    fs::write(&hosts, format!("127.0.0.1:2222 SHA256:{}\n", "x".repeat(43))).unwrap();
    let err = remote::connect(&s, &hosts).err().unwrap().to_string();
    assert!(err.starts_with(remote::CHANGED_HOST), "{err}");
    // Wrong password.
    fs::write(&hosts, format!("127.0.0.1:2222 {fp}\n")).unwrap();
    let mut bad = s.clone();
    bad.password = "nope".into();
    let err = remote::connect(&bad, &hosts).err().unwrap().to_string();
    assert!(err.contains("login"), "{err}");
    fs::write(&hosts, format!("127.0.0.1:2222 {fp}\n")).unwrap();
    exercise("sftp", s, &hosts);
}

#[test]
fn wrong_logins_are_explained() {
    for (protocol, port) in [(Protocol::Ftp, 2121), (Protocol::Dav, 8081)] {
        if !up(port) {
            continue;
        }
        let mut s = server(protocol, port);
        s.password = "wrong".into();
        let tmp = tempfile::tempdir().unwrap();
        let err = remote::connect(&s, &tmp.path().join("h")).err().expect("wrong password must fail").to_string();
        assert!(err.contains("login") || err.contains("password"), "{protocol:?}: {err}");
    }
}
