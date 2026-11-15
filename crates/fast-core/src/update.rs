//! Updating the app from the builds its repository's workflow publishes. This file holds what
//! needs no network and no window: which build belongs to this computer, whether a build is newer
//! than the one running, where the installer is inside a downloaded build, and the little script that
//! swaps the files and starts the app again. The shell (src-tauri/src/update.rs) does the downloading.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DEFAULT_REPO: &str = "enVIdPluginSuite/file-explorer";
pub const DEFAULT_BRANCH: &str = "main";

/// The name the workflow gives the build for this kind of computer (see `.github/workflows/build.yml`).
pub fn artifact_name() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some(if cfg!(target_arch = "aarch64") { "fast-file-explorer-build-windows-arm64" } else { "fast-file-explorer-build-windows-x64" })
    } else if cfg!(target_os = "macos") {
        Some("fast-file-explorer-build-macos-universal")
    } else if cfg!(target_os = "linux") {
        Some(if cfg!(target_arch = "aarch64") { "fast-file-explorer-build-linux-arm64" } else { "fast-file-explorer-build-linux-x64" })
    } else {
        None
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Latest {
    pub run_id: u64,
    pub artifact_id: u64,
    pub sha: String,
    pub branch: String,
    pub created_at: String,
    pub size: u64,
    pub name: String,
    /// Needs a token to fetch, even for a public repository.
    pub download_url: String,
    /// "sha256:..." when GitHub reports it.
    pub digest: Option<String>,
}

/// The newest unexpired build called `name` that was made from `branch`, out of the answer to
/// `GET /repos/{repo}/actions/artifacts?name={name}`. GitHub lists newest first; this does not rely on it.
pub fn pick_latest(listing: &Value, name: &str, branch: &str) -> Option<Latest> {
    let mut best: Option<Latest> = None;
    for a in listing.get("artifacts")?.as_array()? {
        if a.get("name").and_then(Value::as_str) != Some(name) || a.get("expired").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let run = a.get("workflow_run")?;
        if run.get("head_branch").and_then(Value::as_str) != Some(branch) {
            continue;
        }
        let created = a.get("created_at").and_then(Value::as_str).unwrap_or("").to_string();
        let item = Latest {
            run_id: run.get("id").and_then(Value::as_u64).unwrap_or(0),
            artifact_id: a.get("id").and_then(Value::as_u64)?,
            sha: run.get("head_sha").and_then(Value::as_str)?.to_string(),
            branch: branch.to_string(),
            created_at: created.clone(),
            size: a.get("size_in_bytes").and_then(Value::as_u64).unwrap_or(0),
            name: name.to_string(),
            download_url: a.get("archive_download_url").and_then(Value::as_str)?.to_string(),
            digest: a.get("digest").and_then(Value::as_str).map(str::to_string),
        };
        // ISO 8601 timestamps in one format sort as text.
        if best.as_ref().is_none_or(|b| created > b.created_at) {
            best = Some(item);
        }
    }
    best
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Newer {
    Yes,
    No,
    /// This build does not know which commit it was made from (a build made by hand).
    Unknown,
}

pub fn is_newer(current_sha: Option<&str>, latest_sha: &str) -> Newer {
    match current_sha.map(str::trim).filter(|s| !s.is_empty()) {
        None => Newer::Unknown,
        Some(c) if c.eq_ignore_ascii_case(latest_sha.trim()) => Newer::No,
        Some(_) => Newer::Yes,
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InstallerKind {
    /// Windows setup program: installs without asking when given `/S`.
    Nsis,
    Msi,
    AppImage,
    Deb,
    Rpm,
    Dmg,
}

/// The installer inside a downloaded build, preferring the kind that can be applied without questions.
pub fn find_installer(dir: &Path) -> Option<(PathBuf, InstallerKind)> {
    let mut found: Vec<(PathBuf, InstallerKind)> = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, InstallerKind)>, depth: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                walk(&p, out, depth + 1);
                continue;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let kind = if name.ends_with(".exe") && (name.contains("setup") || name.contains("install")) {
                InstallerKind::Nsis
            } else if name.ends_with(".msi") {
                InstallerKind::Msi
            } else if name.ends_with(".appimage") {
                InstallerKind::AppImage
            } else if name.ends_with(".deb") {
                InstallerKind::Deb
            } else if name.ends_with(".rpm") {
                InstallerKind::Rpm
            } else if name.ends_with(".dmg") {
                InstallerKind::Dmg
            } else {
                continue;
            };
            out.push((p, kind));
        }
    }
    walk(dir, &mut found, 0);
    let rank = |k: InstallerKind| match k {
        InstallerKind::Nsis => 0,
        InstallerKind::AppImage => 1,
        InstallerKind::Dmg => 2,
        InstallerKind::Msi => 3,
        InstallerKind::Deb => 4,
        InstallerKind::Rpm => 5,
    };
    found.sort_by_key(|(p, k)| (rank(*k), p.clone()));
    found.into_iter().next()
}

/// "0.2.0" out of `fast-file-explorer_0.2.0_x64-setup.exe` and similar names.
pub fn version_in_name(name: &str) -> Option<String> {
    name.split(['_', '-']).find(|part| {
        let mut dots = part.split('.');
        let (a, b, c) = (dots.next(), dots.next(), dots.next());
        matches!((a, b, c), (Some(a), Some(b), Some(c)) if [a, b, c.trim_end_matches(|ch: char| !ch.is_ascii_digit())].iter().all(|x| !x.is_empty() && x.chars().all(|ch| ch.is_ascii_digit())))
    }).map(|s| s.trim_end_matches(|ch: char| !ch.is_ascii_digit() && ch != '.').to_string())
}

/// Is `a` a newer "x.y.z" than `b`? Unreadable versions are never newer.
pub fn version_newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim_start_matches('v').split('.').map(|p| p.split(['-', '+']).next().unwrap_or("").parse().ok()).collect() };
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The text of the script that finishes an update after the app has closed: install, then start the
/// app again. Windows: a batch file that runs the setup program silently. Returns (file name, text).
#[cfg(any(windows, test))]
pub fn windows_script(installer: &Path, kind: InstallerKind, app_exe: &Path) -> String {
    let install = match kind {
        InstallerKind::Msi => format!("msiexec /i \"{}\" /passive /norestart", installer.display()),
        _ => format!("\"{}\" /S", installer.display()),
    };
    format!(
        "@echo off\r\nrem Finishes an update of FAST File Explorer.\r\nping -n 3 127.0.0.1 >nul\r\n{install}\r\nstart \"\" \"{app}\"\r\ndel \"%~f0\"\r\n",
        app = app_exe.display()
    )
}

/// POSIX shell text that swaps a running AppImage for the new one and starts it again.
pub fn appimage_script(new_file: &Path, target: &Path) -> String {
    let q = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
    format!(
        "sleep 1\ncp -f {new} {tmp} && chmod +x {tmp} && mv -f {tmp} {target}\nexec {target}\n",
        new = q(new_file),
        tmp = q(&PathBuf::from(format!("{}.new", target.display()))),
        target = q(target)
    )
}

/// Unpack a downloaded build into `into`. Names that would land outside it are refused.
pub fn unzip(zip_file: &Path, into: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_file)?).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(into)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(std::io::Error::other)?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the build contains a path outside its folder"));
        };
        let out = into.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
    }
    Ok(())
}

/// "sha256:ab12..." against a computed hash, ignoring case. A missing digest passes: there is nothing to check.
pub fn digest_matches(digest: Option<&str>, actual_hex: &str) -> bool {
    match digest {
        None => true,
        Some(d) => d.strip_prefix("sha256:").unwrap_or(d).eq_ignore_ascii_case(actual_hex),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn art(id: u64, name: &str, branch: &str, sha: &str, created: &str, expired: bool) -> Value {
        json!({"id": id, "name": name, "size_in_bytes": 1000 + id, "archive_download_url": format!("https://api.github.com/x/{id}/zip"), "expired": expired, "created_at": created, "digest": "sha256:ABCD", "workflow_run": {"id": 70 + id, "head_branch": branch, "head_sha": sha}})
    }

    #[test]
    fn picks_the_newest_build_for_this_branch() {
        let n = "fast-file-explorer-build-windows-x64";
        let list = json!({"artifacts": [
            art(1, n, "main", "aaa", "2026-11-14T10:00:00Z", false),
            art(2, n, "feature", "bbb", "2026-11-15T10:00:00Z", false),
            art(3, "other", "main", "ccc", "2026-11-15T11:00:00Z", false),
            art(4, n, "main", "ddd", "2026-11-15T09:00:00Z", false),
            art(5, n, "main", "eee", "2026-11-15T12:00:00Z", true),
        ]});
        let l = pick_latest(&list, n, "main").unwrap();
        assert_eq!((l.sha.as_str(), l.artifact_id, l.run_id), ("ddd", 4, 74), "newest unexpired build of that name on that branch");
        assert_eq!(l.digest.as_deref(), Some("sha256:ABCD"));
        assert!(pick_latest(&list, n, "nope").is_none());
        assert!(pick_latest(&json!({"message": "Not Found"}), n, "main").is_none());
    }

    #[test]
    fn compares_builds() {
        assert_eq!(is_newer(Some("abc"), "abc"), Newer::No);
        assert_eq!(is_newer(Some("ABC"), "abc"), Newer::No);
        assert_eq!(is_newer(Some("abc"), "def"), Newer::Yes);
        assert_eq!(is_newer(None, "def"), Newer::Unknown);
        assert_eq!(is_newer(Some(" "), "def"), Newer::Unknown);
        assert!(version_newer("0.2.0", "0.1.9") && version_newer("v1.0.0", "0.9.9") && !version_newer("0.2.0", "0.2.0") && !version_newer("x", "0.1.0"));
        assert!(version_newer("0.10.0", "0.9.0"), "numbers, not text");
    }

    #[test]
    fn finds_the_installer_that_needs_no_questions() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("nsis")).unwrap();
        std::fs::create_dir_all(d.path().join("msi")).unwrap();
        std::fs::write(d.path().join("msi/fast-file-explorer_0.2.0_x64_en-US.msi"), b"m").unwrap();
        std::fs::write(d.path().join("nsis/fast-file-explorer_0.2.0_x64-setup.exe"), b"e").unwrap();
        std::fs::write(d.path().join("readme.txt"), b"x").unwrap();
        let (p, k) = find_installer(d.path()).unwrap();
        assert_eq!(k, InstallerKind::Nsis);
        assert!(p.ends_with("fast-file-explorer_0.2.0_x64-setup.exe"));
        std::fs::remove_file(&p).unwrap();
        assert_eq!(find_installer(d.path()).unwrap().1, InstallerKind::Msi);
        let e = tempfile::tempdir().unwrap();
        assert!(find_installer(e.path()).is_none());
    }

    #[test]
    fn reads_the_version_out_of_an_installer_name() {
        assert_eq!(version_in_name("fast-file-explorer_0.2.0_x64-setup.exe").as_deref(), Some("0.2.0"));
        assert_eq!(version_in_name("FAST_File_Explorer_1.10.3_amd64.AppImage").as_deref(), Some("1.10.3"));
        assert_eq!(version_in_name("setup.exe"), None);
    }

    #[test]
    fn scripts_install_then_start_the_app() {
        let s = windows_script(Path::new(r"C:\tmp\fast setup.exe"), InstallerKind::Nsis, Path::new(r"C:\Apps\FAST\fast.exe"));
        assert!(s.contains("\"C:\\tmp\\fast setup.exe\" /S") && s.contains("start \"\" \"C:\\Apps\\FAST\\fast.exe\""), "{s}");
        assert!(s.find("/S").unwrap() < s.find("start \"\"").unwrap());
        assert!(windows_script(Path::new("a.msi"), InstallerKind::Msi, Path::new("x.exe")).contains("msiexec /i"));
        let a = appimage_script(Path::new("/tmp/it's new.AppImage"), Path::new("/home/me/FAST.AppImage"));
        assert!(a.contains("'/tmp/it'\\''s new.AppImage'") && a.contains("exec '/home/me/FAST.AppImage'"), "{a}");
    }

    #[test]
    fn unpacks_a_build_and_refuses_escaping_names() {
        use std::io::Write;
        let d = tempfile::tempdir().unwrap();
        let zip_path = d.path().join("build.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.start_file("bundle/nsis/app-setup.exe", o).unwrap();
            w.write_all(b"installer").unwrap();
            w.finish().unwrap();
        }
        let out = d.path().join("out");
        unzip(&zip_path, &out).unwrap();
        assert_eq!(std::fs::read(out.join("bundle/nsis/app-setup.exe")).unwrap(), b"installer");
        assert_eq!(find_installer(&out).unwrap().1, InstallerKind::Nsis);
        let evil = d.path().join("evil.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&evil).unwrap());
            w.start_file("../escape.txt", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        assert!(unzip(&evil, &d.path().join("out2")).is_err());
        assert!(!d.path().join("escape.txt").exists());
    }

    #[test]
    fn digests() {
        assert!(digest_matches(None, "ff"));
        assert!(digest_matches(Some("sha256:FF"), "ff"));
        assert!(!digest_matches(Some("sha256:aa"), "ff"));
    }
}
