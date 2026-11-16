//! Commands behind the Tools dialogs and the file viewer. Long work (searching inside files,
//! finding duplicates, comparing folders, converting pictures…) runs as a cancellable job whose
//! progress arrives as `tool-progress` events and, for the search, results as `tool-hits` events.

use crate::commands::{guard, CmdResult};
use crate::state::Shared;
use base64::Engine;
use fast_core::filetools as ft;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

static RUNNING: std::sync::LazyLock<Mutex<HashMap<u64, Arc<AtomicBool>>>> = std::sync::LazyLock::new(Default::default);

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToolSpec {
    #[serde(rename_all = "camelCase")]
    Split { src: String, dest_dir: String, part_size: u64 },
    #[serde(rename_all = "camelCase")]
    Join { first: String, dest_dir: String },
    #[serde(rename_all = "camelCase")]
    Encode { src: String, dest_dir: String, encoding: String },
    #[serde(rename_all = "camelCase")]
    Decode { src: String, dest_dir: String },
    #[serde(rename_all = "camelCase")]
    CompareFiles { a: String, b: String, #[serde(default)] opts: ft::CompareOpts },
    #[serde(rename_all = "camelCase")]
    CompareDirs { left: String, right: String, #[serde(default)] opts: ft::DirOpts },
    #[serde(rename_all = "camelCase")]
    Duplicates { roots: Vec<String>, #[serde(default)] opts: ft::DupOpts },
    #[serde(rename_all = "camelCase")]
    Grep { roots: Vec<String>, opts: ft::GrepOpts },
    #[serde(rename_all = "camelCase")]
    Convert { paths: Vec<String>, opts: ft::ConvertOpts },
}

impl ToolSpec {
    fn paths(&self) -> Vec<&str> {
        match self {
            ToolSpec::Split { src, dest_dir, .. } | ToolSpec::Encode { src, dest_dir, .. } | ToolSpec::Decode { src, dest_dir } => vec![src, dest_dir],
            ToolSpec::Join { first, dest_dir } => vec![first, dest_dir],
            ToolSpec::CompareFiles { a, b, .. } => vec![a, b],
            ToolSpec::CompareDirs { left, right, .. } => vec![left, right],
            ToolSpec::Duplicates { roots, .. } | ToolSpec::Grep { roots, .. } => roots.iter().map(String::as_str).collect(),
            ToolSpec::Convert { paths, .. } => paths.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Serialize, Clone)]
struct ToolProgress {
    id: u64,
    done: u64,
    total: u64,
    note: String,
}
#[derive(Serialize, Clone)]
struct ToolHits {
    id: u64,
    hits: Vec<ft::GrepHit>,
}

fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn run(app: &AppHandle, id: u64, spec: ToolSpec, cancel: &AtomicBool, stage_root: &Path) -> Result<serde_json::Value, String> {
    use serde_json::to_value;
    let emitter = app.clone();
    let progress = move |done: u64, total: u64, note: &str| {
        let _ = emitter.emit("tool-progress", ToolProgress { id, done, total, note: note.to_string() });
    };
    let v = match spec {
        ToolSpec::Split { src, dest_dir, part_size } => {
            let parts = ft::split_file(&pb(&src), &pb(&dest_dir), part_size, cancel, &progress)?;
            to_value(parts.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>())
        }
        ToolSpec::Join { first, dest_dir } => {
            let (out, ok) = ft::join_files(&pb(&first), &pb(&dest_dir), cancel, &progress)?;
            to_value(serde_json::json!({ "path": out.to_string_lossy(), "verified": ok }))
        }
        ToolSpec::Encode { src, dest_dir, encoding } => to_value(ft::encode_file(&pb(&src), &pb(&dest_dir), &encoding)?.to_string_lossy()),
        ToolSpec::Decode { src, dest_dir } => to_value(ft::decode_file(&pb(&src), &pb(&dest_dir))?.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>()),
        ToolSpec::CompareFiles { a, b, opts } => to_value(ft::compare_files(&pb(&a), &pb(&b), &opts, cancel)?),
        ToolSpec::CompareDirs { left, right, mut opts } => {
            // An archive on either side is unpacked to a temporary folder (with the dates it holds, for zip) and compared as one.
            let mut stage = |side: &str, p: &str| -> Result<(PathBuf, Option<String>), String> {
                let path = pb(p);
                if path.is_file() && (fast_core::archive::format_of(&path).is_some() || fast_core::browse::kind_of(&path).is_some()) {
                    let dir = stage_root.join(format!("cmp-{id}-{side}"));
                    progress(0, 0, &format!("Unpacking {}…", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    if !ft::stage_archive(&path, &dir, cancel, &progress)? {
                        opts.by_content = true; // this kind of archive keeps no dates
                    }
                    Ok((dir, Some(p.to_string())))
                } else {
                    Ok((path, None))
                }
            };
            let (l, la) = stage("left", &left)?;
            let (r, ra) = stage("right", &right)?;
            let result = ft::compare_dirs(&l, &r, &opts, cancel, &progress)?;
            let mut v = to_value(result).map_err(|e| e.to_string())?;
            v["leftRoot"] = l.to_string_lossy().into();
            v["rightRoot"] = r.to_string_lossy().into();
            v["leftArchive"] = la.into();
            v["rightArchive"] = ra.into();
            Ok(v)
        }
        ToolSpec::Duplicates { roots, opts } => to_value(ft::find_duplicates(&roots.iter().map(|r| pb(r)).collect::<Vec<_>>(), &opts, cancel, &progress)?),
        ToolSpec::Grep { roots, opts } => {
            let hits_app = app.clone();
            let summary = ft::grep(&roots.iter().map(|r| pb(r)).collect::<Vec<_>>(), &opts, cancel, &mut |hits| {
                let _ = hits_app.emit("tool-hits", ToolHits { id, hits });
            }, &progress)?;
            to_value(summary)
        }
        ToolSpec::Convert { paths, opts } => to_value(ft::convert_images(&paths.iter().map(|r| pb(r)).collect::<Vec<_>>(), &opts, cancel, &progress)?),
    };
    v.map_err(|e| e.to_string())
}

/// Run one tool and wait for it. `id` is chosen by the caller so it can cancel and follow the events.
#[tauri::command]
pub async fn tool_run(app: AppHandle, state: State<'_, Shared>, id: u64, spec: ToolSpec) -> CmdResult<serde_json::Value> {
    for p in spec.paths() {
        if fast_core::remote::is_remote(p) || fast_core::browse::is_loc(p) {
            return Err("These tools work on files on this computer. Copy the files here first.".into());
        }
        guard(&state, p)?;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    RUNNING.lock().insert(id, cancel.clone());
    let stage_root = state.paths.remote_tmp.clone();
    crate::remote::sweep_staging(&stage_root, "cmp-", Some(std::time::Duration::from_secs(6 * 3600)));
    let out = tauri::async_runtime::spawn_blocking(move || {
        let r = run(&app, id, spec, &cancel, &stage_root);
        RUNNING.lock().remove(&id);
        r
    })
    .await
    .map_err(|e| e.to_string())?;
    out
}

#[tauri::command]
pub fn tool_cancel(id: u64) {
    if let Some(c) = RUNNING.lock().get(&id) {
        c.store(true, Ordering::Relaxed);
    }
}

// ---- The viewer --------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewInfo {
    pub name: String,
    pub size: u64,
    pub modified: Option<i64>,
}

#[tauri::command]
pub async fn view_info(state: State<'_, Shared>, path: String) -> CmdResult<ViewInfo> {
    guard(&state, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let p = Path::new(&path);
        let m = std::fs::metadata(p).map_err(|e| e.to_string())?;
        if !m.is_file() {
            return Err("Only files can be viewed.".into());
        }
        let modified = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
        Ok(ViewInfo { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), size: m.len(), modified })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A piece of a file as Base64 (at most 4 MB).
#[tauri::command]
pub async fn view_read(state: State<'_, Shared>, path: String, offset: u64, len: usize) -> CmdResult<String> {
    guard(&state, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = ft::view_read(Path::new(&path), offset, len)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn view_find(state: State<'_, Shared>, id: u64, path: String, needle: String, start: u64, backward: bool, ascii_ci: bool) -> CmdResult<Option<u64>> {
    guard(&state, &path)?;
    let cancel = Arc::new(AtomicBool::new(false));
    RUNNING.lock().insert(id, cancel.clone());
    let r = tauri::async_runtime::spawn_blocking(move || {
        let needle = base64::engine::general_purpose::STANDARD.decode(needle).map_err(|e| e.to_string())?;
        let r = ft::view_find(Path::new(&path), &needle, start, backward, ascii_ci, &cancel);
        RUNNING.lock().remove(&id);
        r
    })
    .await
    .map_err(|e| e.to_string())?;
    r
}

/// Hand a quick look to QuickLook (Windows), Seer (Windows), the system's preview (`qlmanage` on macOS, Sushi on
/// GNOME) when the person chose that and it is installed. `false` means "not available": the built-in viewer is used.
#[tauri::command]
pub fn preview_external(path: String, engine: String) -> bool {
    use std::process::{Command, Stdio};
    let p = Path::new(&path);
    if !p.is_file() {
        return false;
    }
    let spawn = |mut c: Command| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok();
    let first_existing = |list: Vec<std::path::PathBuf>| list.into_iter().find(|x| x.is_file());
    match engine.as_str() {
        "quicklook" => {
            if cfg!(windows) {
                let local = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
                let pf = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from);
                let mut cands = vec![];
                if let Some(l) = &local {
                    cands.push(l.join("Programs/QuickLook/QuickLook.exe"));
                }
                if let Some(x) = &pf {
                    cands.push(x.join("QuickLook/QuickLook.exe"));
                }
                // The Microsoft Store build is started through its alias.
                if let Some(l) = &local {
                    cands.push(l.join("Microsoft/WindowsApps/QuickLook.exe"));
                }
                match first_existing(cands) {
                    Some(exe) => spawn({
                        let mut c = Command::new(exe);
                        c.arg(&path);
                        c
                    }),
                    None => false,
                }
            } else {
                false
            }
        }
        "seer" => {
            if cfg!(windows) {
                let local = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
                let pf = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from);
                let mut cands = vec![];
                if let Some(l) = &local {
                    cands.push(l.join("Programs/Seer/Seer.exe"));
                }
                if let Some(x) = &pf {
                    cands.push(x.join("Seer/Seer.exe"));
                }
                match first_existing(cands) {
                    Some(exe) => spawn({
                        let mut c = Command::new(exe);
                        c.arg(&path);
                        c
                    }),
                    None => false,
                }
            } else {
                false
            }
        }
        "system" => {
            if cfg!(target_os = "macos") {
                spawn({
                    let mut c = Command::new("qlmanage");
                    c.args(["-p", &path]);
                    c
                })
            } else if cfg!(target_os = "linux") {
                spawn({
                    let mut c = Command::new("sushi");
                    c.arg(&path);
                    c
                })
            } else {
                false
            }
        }
        _ => false,
    }
}
