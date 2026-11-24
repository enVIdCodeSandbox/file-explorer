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

impl Latest {
    /// Names this build: its commit when known (the same build found in two places has one name),
    /// otherwise its release version.
    pub fn key(&self) -> String {
        if !self.sha.is_empty() { self.sha.clone() } else { format!("release-{}", self.version.as_deref().unwrap_or("?")) }
    }
}

/// Where a build was found.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// An artifact of the workflow run (downloading needs a token or nightly.link).
    #[default]
    Actions,
    /// An installer attached to a published release (public, no token).
    Release,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Latest {
    pub source: Source,
    /// "0.2.1" for a release.
    pub version: Option<String>,
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
            source: Source::Actions,
            version: None,
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

/// What kind of installer a file is, by its name.
pub fn kind_of_name(name: &str) -> Option<InstallerKind> {
    let name = name.to_lowercase();
    Some(if name.ends_with(".exe") && (name.contains("setup") || name.contains("install")) {
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
        return None;
    })
}

/// What this computer is, for choosing among the files of a release.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub os: &'static str,
    pub arm: bool,
    /// Running from an AppImage: that kind can replace itself, the others need the system installer.
    pub appimage: bool,
}

impl Target {
    pub fn here() -> Target {
        Target {
            os: if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" },
            arm: cfg!(target_arch = "aarch64"),
            appimage: std::env::var_os("APPIMAGE").is_some(),
        }
    }
}

/// Does this file name belong to this kind of processor? Names say "x64", "amd64" or "x86_64" for
/// Intel and AMD, "arm64" or "aarch64" for ARM, "universal" for both (macOS).
fn fits_arch(name: &str, arm: bool) -> bool {
    let n = name.to_lowercase();
    if n.contains("universal") {
        return true;
    }
    let is_arm = n.contains("arm64") || n.contains("aarch64");
    let is_x64 = n.contains("x64") || n.contains("amd64") || n.contains("x86_64");
    if arm { is_arm || !is_x64 } else { is_x64 || !is_arm }
}

/// The best installer among a release's files for this computer: the kind that installs without
/// questions first.
pub fn pick_asset<'a>(assets: &'a [Value], t: Target) -> Option<&'a Value> {
    let rank = |k: InstallerKind| -> Option<u8> {
        match (t.os, k) {
            ("windows", InstallerKind::Nsis) => Some(0),
            ("windows", InstallerKind::Msi) => Some(1),
            ("linux", InstallerKind::AppImage) => Some(if t.appimage { 0 } else { 2 }),
            ("linux", InstallerKind::Deb) => Some(1),
            ("linux", InstallerKind::Rpm) => Some(3),
            ("macos", InstallerKind::Dmg) => Some(0),
            _ => None,
        }
    };
    assets
        .iter()
        .filter_map(|a| {
            let name = a.get("name")?.as_str()?;
            let r = rank(kind_of_name(name)?)?;
            fits_arch(name, t.arm).then_some((r, a))
        })
        .min_by_key(|(r, _)| *r)
        .map(|(_, a)| a)
}

/// The newest published release (not a draft or pre-release) that has an installer for this computer, out of
/// the answer to `GET /repos/{repo}/releases`.
pub fn pick_release(listing: &Value, t: Target) -> Option<Latest> {
    let mut best: Option<Latest> = None;
    for r in listing.as_array()? {
        if r.get("draft").and_then(Value::as_bool).unwrap_or(false) || r.get("prerelease").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(assets) = r.get("assets").and_then(Value::as_array) else { continue };
        let Some(asset) = pick_asset(assets, t) else { continue };
        let tag = r.get("tag_name").and_then(Value::as_str).unwrap_or("");
        let commit = r.get("target_commitish").and_then(Value::as_str).unwrap_or("");
        let created = r.get("published_at").or_else(|| r.get("created_at")).and_then(Value::as_str).unwrap_or("").to_string();
        let item = Latest {
            source: Source::Release,
            version: Some(tag.trim_start_matches('v').to_string()).filter(|v| !v.is_empty()),
            run_id: 0,
            artifact_id: asset.get("id").and_then(Value::as_u64).unwrap_or(0),
            // Only a full commit id says which commit it was; a branch name does not.
            sha: if commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()) { commit.to_string() } else { String::new() },
            branch: commit.to_string(),
            created_at: created.clone(),
            size: asset.get("size").and_then(Value::as_u64).unwrap_or(0),
            name: asset.get("name").and_then(Value::as_str)?.to_string(),
            download_url: asset.get("browser_download_url").and_then(Value::as_str)?.to_string(),
            digest: asset.get("digest").and_then(Value::as_str).map(str::to_string),
        };
        if best.as_ref().is_none_or(|b| created > b.created_at) {
            best = Some(item);
        }
    }
    best
}

/// Is `latest` newer than the build that is running? A commit id decides when both sides have one;
/// otherwise a release is compared by its version number.
pub fn newer_than_running(current_sha: Option<&str>, current_version: &str, built_at: Option<u64>, latest: &Latest) -> Newer {
    // A release has a version number, and a version has an order. A commit id only says "same or different",
    // so using it here offered an older release as an update to a newer running build, which then updated
    // itself back up from the newer build, over and over.
    if let (Source::Release, Some(v)) = (&latest.source, &latest.version) {
        if version_parses(v) && version_parses(current_version) {
            // A newer version number published before this build was even made is a mislabelled old one.
            return if version_newer(v, current_version) && !made_before(&latest.created_at, built_at) { Newer::Yes } else { Newer::No };
        }
    }
    if !latest.sha.is_empty() {
        let by_sha = is_newer(current_sha, &latest.sha);
        if by_sha == Newer::Yes && made_before(&latest.created_at, built_at) {
            // A different build, but older than this one (an old workflow run that is still kept).
            return Newer::No;
        }
        if by_sha != Newer::Unknown {
            return by_sha;
        }
    }
    match (&latest.source, &latest.version) {
        (Source::Release, Some(v)) => {
            if version_newer(v, current_version) {
                Newer::Yes
            } else {
                Newer::No
            }
        }
        _ => Newer::Unknown,
    }
}

/// The build to offer out of everything found: the newest one that is newer than the running build,
/// and if none is, the newest one at all (to show what the newest is). On a tie a release wins,
/// because it can be downloaded without a token.
pub fn best_of(found: Vec<Latest>, current_sha: Option<&str>, current_version: &str, built_at: Option<u64>) -> Option<(Latest, Newer)> {
    let mut judged: Vec<(Latest, Newer)> = found.into_iter().map(|l| { let n = newer_than_running(current_sha, current_version, built_at, &l); (l, n) }).collect();
    let key = |(l, n): &(Latest, Newer)| (matches!(n, Newer::Yes | Newer::Unknown), l.created_at.clone(), l.source == Source::Release);
    judged.sort_by_key(|j| key(j));
    judged.pop()
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
            let Some(kind) = kind_of_name(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) else { continue };
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

/// Was something published (RFC 3339, UTC, like `2026-11-15T20:39:00Z`) before the running build was made
/// (seconds since 1970)? False when either date is not known.
pub fn made_before(created_at: &str, built_at: Option<u64>) -> bool {
    let Some(built) = built_at else { return false };
    let c = created_at.trim();
    if c.len() < 19 || !c.is_char_boundary(19) {
        return false;
    }
    let b = crate::crashlog::stamp(built).replacen(' ', "T", 1);
    c[..19] < *b.as_str()
}

fn version_parses(v: &str) -> bool {
    v.trim_start_matches('v').split('.').all(|p| p.split(['-', '+']).next().unwrap_or("").parse::<u64>().is_ok())
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
pub fn windows_script(installer: &Path, kind: InstallerKind, app_exe: &Path, result_file: &Path) -> String {
    let install = match kind {
        InstallerKind::Msi => format!("msiexec /i \"{}\" /passive /norestart", installer.display()),
        _ => format!("\"{}\" /S", installer.display()),
    };
    // The installer's exit code is written down (the redirection comes first: "echo 1> file" would redirect handle 1),
    // so that the app can say what happened if it comes back as the same build. The wait lets the old app finish closing.
    format!(
        "@echo off\r\nrem Finishes an update of FAST File Explorer.\r\nping -n 7 127.0.0.1 >nul\r\n{install}\r\n>\"{result}\" echo %errorlevel%\r\nstart \"\" \"{app}\"\r\ndel \"%~f0\"\r\n",
        app = app_exe.display(),
        result = result_file.display()
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

/// POSIX shell text that replaces a Mac app with the one inside a downloaded disk image and starts it again. It runs
/// after the app has quit. Nothing is left half done: the old app is moved aside and put back if the copy fails, and
/// whatever happens the person ends up with an app that opens (the old one, and the disk image shown for a manual
/// install). The result is written to `result_file` ("0" for success).
pub fn macos_script(dmg: &Path, app: &Path, result_file: &Path) -> String {
    let q = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
    let (dmg_q, app_q, result_q) = (q(dmg), q(app), q(result_file));
    let new_q = q(&PathBuf::from(format!("{}.new", app.display())));
    let old_q = q(&PathBuf::from(format!("{}.old", app.display())));
    format!(
        r#"sleep 1
RESULT={result_q}
MNT=$(mktemp -d /tmp/fast-update.XXXXXX)
finish() {{
  hdiutil detach "$MNT" -force >/dev/null 2>&1
  rmdir "$MNT" 2>/dev/null
}}
fail() {{
  echo "$1" > "$RESULT"
  finish
  rm -rf {new_q}
  /usr/bin/open -n {app_q}
  /usr/bin/open {dmg_q}
  exit 1
}}
hdiutil attach -nobrowse -noverify -noautoopen -readonly -mountpoint "$MNT" {dmg_q} >/dev/null 2>&1 || fail 2
NEW=$(ls -d "$MNT"/*.app 2>/dev/null | head -n 1)
[ -n "$NEW" ] || fail 3
rm -rf {new_q}
ditto "$NEW" {new_q} || fail 4
xattr -dr com.apple.quarantine {new_q} >/dev/null 2>&1
rm -rf {old_q}
if [ -e {app_q} ]; then
  mv {app_q} {old_q} || fail 5
fi
if ! mv {new_q} {app_q}; then
  [ -e {old_q} ] && mv {old_q} {app_q}
  fail 6
fi
rm -rf {old_q}
echo 0 > "$RESULT"
finish
exec /usr/bin/open -n {app_q}
"#
    )
}

/// Unpack a downloaded build into `into`. Names that would land outside it are refused.
pub fn unzip(zip_file: &Path, into: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_file)?).map_err(std::io::Error::other)?;
    // Local headers must agree with the central directory, so what is checked is what is written.
    archive.validate_header_consistency().map_err(std::io::Error::other)?;
    std::fs::create_dir_all(into)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index_with_options(i, crate::archive::zip_read_options()).map_err(std::io::Error::other)?;
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

    fn release(tag: &str, commit: &str, at: &str, files: &[&str]) -> Value {
        json!({"tag_name": tag, "target_commitish": commit, "draft": false, "prerelease": false, "published_at": at,
            "assets": files.iter().enumerate().map(|(i, f)| json!({"id": i + 1, "name": f, "size": 5000, "digest": "sha256:AB", "browser_download_url": format!("https://github.com/o/r/releases/download/{tag}/{f}")})).collect::<Vec<_>>()})
    }
    const FILES: &[&str] = &["FAST.File.Explorer_0.3.0_x64-setup.exe", "FAST.File.Explorer_0.3.0_x64_en-US.msi", "FAST.File.Explorer_0.3.0_arm64-setup.exe", "FAST.File.Explorer_0.3.0_amd64.AppImage", "FAST.File.Explorer_0.3.0_amd64.deb", "FAST.File.Explorer-0.3.0-1.x86_64.rpm", "FAST.File.Explorer_0.3.0_aarch64.AppImage", "FAST.File.Explorer_0.3.0_universal.dmg", "notes.txt"];

    #[test]
    fn picks_the_right_file_of_a_release() {
        let assets = release("v0.3.0", "main", "2026-11-15T10:00:00Z", FILES)["assets"].as_array().unwrap().clone();
        let name = |t: Target| pick_asset(&assets, t).unwrap()["name"].as_str().unwrap().to_string();
        assert_eq!(name(Target { os: "windows", arm: false, appimage: false }), "FAST.File.Explorer_0.3.0_x64-setup.exe");
        assert_eq!(name(Target { os: "windows", arm: true, appimage: false }), "FAST.File.Explorer_0.3.0_arm64-setup.exe");
        assert_eq!(name(Target { os: "linux", arm: false, appimage: true }), "FAST.File.Explorer_0.3.0_amd64.AppImage");
        assert_eq!(name(Target { os: "linux", arm: false, appimage: false }), "FAST.File.Explorer_0.3.0_amd64.deb");
        assert_eq!(name(Target { os: "linux", arm: true, appimage: true }), "FAST.File.Explorer_0.3.0_aarch64.AppImage");
        assert_eq!(name(Target { os: "macos", arm: true, appimage: false }), "FAST.File.Explorer_0.3.0_universal.dmg");
        assert!(pick_asset(&assets[8..], Target { os: "windows", arm: false, appimage: false }).is_none());
    }

    #[test]
    fn picks_the_newest_published_release() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let list = json!([
            {"tag_name": "v0.9.0", "draft": true, "prerelease": false, "assets": release("v0.9.0", "main", "2026-11-16T00:00:00Z", FILES)["assets"]},
            {"tag_name": "v0.8.0", "draft": false, "prerelease": true, "published_at": "2026-11-16T00:00:00Z", "assets": release("v0.8.0", "main", "x", FILES)["assets"]},
            release("v0.3.0", sha, "2026-11-15T10:00:00Z", FILES),
            release("v0.2.0", "main", "2026-11-10T10:00:00Z", FILES),
            release("v0.4.0", "main", "2026-11-17T10:00:00Z", &["only-a-source.zip"]),
        ]);
        let t = Target { os: "windows", arm: false, appimage: false };
        let l = pick_release(&list, t).unwrap();
        assert_eq!((l.source, l.version.as_deref(), l.sha.as_str()), (Source::Release, Some("0.3.0"), sha), "no drafts, pre-releases or releases without an installer");
        assert!(l.download_url.ends_with("x64-setup.exe") && l.name.ends_with("x64-setup.exe"));
        assert_eq!(pick_release(&json!([release("v0.2.0", "main", "t", FILES)]), t).unwrap().sha, "", "a branch name is not a commit");
        assert!(pick_release(&json!({"message": "Not Found"}), t).is_none());
    }

    #[test]
    fn chooses_among_the_places_it_looked() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let rel = |ver: &str, sha: &str, at: &str| Latest { source: Source::Release, version: Some(ver.into()), run_id: 0, artifact_id: 1, sha: sha.into(), branch: "main".into(), created_at: at.into(), size: 1, name: "x".into(), download_url: "u".into(), digest: None };
        let act = |sha: &str, at: &str| Latest { source: Source::Actions, version: None, ..rel("", sha, at) };
        // By commit id when both sides have one.
        assert_eq!(newer_than_running(Some(&sha_a), "0.2.0", None, &rel("0.2.0", &sha_a, "t")), Newer::No);
        assert_eq!(newer_than_running(Some(&sha_a), "0.2.0", None, &rel("0.2.1", &sha_b, "t")), Newer::Yes);
        // By version when this build does not know its commit, or the release does not name one.
        assert_eq!(newer_than_running(None, "0.2.0", None, &rel("0.2.1", "", "t")), Newer::Yes);
        assert_eq!(newer_than_running(None, "0.2.1", None, &rel("0.2.1", &sha_b, "t")), Newer::No);
        assert_eq!(newer_than_running(None, "0.2.1", None, &act(&sha_b, "t")), Newer::Unknown);
        // An older release is never an update, whatever its commit id (it used to be, and caused an update loop).
        assert_eq!(newer_than_running(Some(&sha_a), "0.10.3", None, &rel("0.10.1", &sha_b, "t")), Newer::No);
        assert_eq!(newer_than_running(Some(&sha_a), "0.10.3", None, &rel("0.10.4", &sha_b, "t")), Newer::Yes);
        // The date counts: a different build that is older than the running one is not an update; a newer one is.
        let built = 1_763_240_000; // 2025-11-15T21:33:20Z
        assert_eq!(newer_than_running(Some(&sha_a), "0.2.0", Some(built), &act(&sha_b, "2025-11-15T20:00:00Z")), Newer::No);
        assert_eq!(newer_than_running(Some(&sha_a), "0.2.0", Some(built), &act(&sha_b, "2025-11-15T22:00:00Z")), Newer::Yes);
        assert_eq!(newer_than_running(Some(&sha_a), "0.2.0", Some(built), &rel("0.2.1", &sha_b, "2025-11-15T20:00:00Z")), Newer::No);
        assert!(made_before("2025-11-15T20:00:00Z", Some(built)) && !made_before("2025-11-15T22:00:00Z", Some(built)) && !made_before("2025-11-15T20:00:00Z", None));
        // The newer of the two places wins; on a tie, the release.
        let (l, n) = best_of(vec![rel("0.3.0", &sha_b, "2026-11-15T10:00:00Z"), act(&"c".repeat(40), "2026-11-15T11:00:00Z")], Some(&sha_a), "0.2.0", None).unwrap();
        assert_eq!((l.source, n), (Source::Actions, Newer::Yes));
        let (l, _) = best_of(vec![act(&sha_b, "2026-11-15T10:00:00Z"), rel("0.3.0", &sha_b, "2026-11-15T10:00:00Z")], Some(&sha_a), "0.2.0", None).unwrap();
        assert_eq!(l.source, Source::Release);
        // Nothing newer: still reports the newest, as "not newer".
        let (l, n) = best_of(vec![rel("0.2.0", &sha_a, "2026-11-15T10:00:00Z")], Some(&sha_a), "0.2.0", None).unwrap();
        assert_eq!((l.version.as_deref(), n), (Some("0.2.0"), Newer::No));
        assert!(best_of(vec![], None, "0.2.0", None).is_none());
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
        let s = windows_script(Path::new(r"C:\tmp\fast setup.exe"), InstallerKind::Nsis, Path::new(r"C:\Apps\FAST\fast.exe"), Path::new(r"C:\tmp\result.txt"));
        assert!(s.contains(">\"C:\\tmp\\result.txt\" echo %errorlevel%"), "{s}");
        assert!(s.find("/S").unwrap() < s.find("echo %errorlevel%").unwrap() && s.find("echo %errorlevel%").unwrap() < s.find("start \"\"").unwrap());
        assert!(s.contains("\"C:\\tmp\\fast setup.exe\" /S") && s.contains("start \"\" \"C:\\Apps\\FAST\\fast.exe\""), "{s}");
        assert!(s.find("/S").unwrap() < s.find("start \"\"").unwrap());
        assert!(windows_script(Path::new("a.msi"), InstallerKind::Msi, Path::new("x.exe"), Path::new("r.txt")).contains("msiexec /i"));
        let m = macos_script(Path::new("/tmp/it's.dmg"), Path::new("/Applications/FAST File Explorer.app"), Path::new("/tmp/r.txt"));
        assert!(m.contains("'/tmp/it'\\''s.dmg'"), "{m}");
        assert!(m.contains("mv '/Applications/FAST File Explorer.app' '/Applications/FAST File Explorer.app.old'"), "{m}");
        assert!(m.contains("exec /usr/bin/open -n '/Applications/FAST File Explorer.app'"), "{m}");
        assert!(m.find("ditto").unwrap() < m.find(".app.old'").unwrap(), "the new copy is made before the old one is touched");
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
