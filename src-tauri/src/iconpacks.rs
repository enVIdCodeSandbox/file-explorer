//! Icon collections that are installed on request. Nothing is bundled with the app: a preview comes over the
//! network (a few kilobytes), installing downloads the collection once into the cache folder, and it can be removed
//! again at any time. The rules for what is kept live in `fast_core::iconpacks`.

use crate::commands::CmdResult;
use crate::state::Shared;
use fast_core::iconpacks as packs;
use serde::Serialize;
use tauri::State;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackList {
    pub packs: Vec<packs::PackState>,
    /// Bytes all installed collections take.
    pub disk: u64,
}

fn list(st: &Shared) -> PackList {
    PackList { packs: packs::states(&st.paths.icon_packs), disk: packs::footprint(&st.paths.icon_packs).0 }
}

#[tauri::command]
pub async fn icon_packs_list(state: State<'_, Shared>) -> CmdResult<PackList> {
    let st = state.inner().clone();
    blocking(move || Ok(list(&st))).await
}

/// An installed collection's icons, as `{"w","h","icons":{name: body | [body, w, h]}}`.
#[tauri::command]
pub async fn icon_pack_read(state: State<'_, Shared>, id: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || packs::read(&st.paths.icon_packs, &id).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn icon_pack_remove(state: State<'_, Shared>, id: String) -> CmdResult<PackList> {
    let st = state.inner().clone();
    blocking(move || {
        packs::remove(&st.paths.icon_packs, &id).map_err(|e| e.to_string())?;
        Ok(list(&st))
    })
    .await
}

/// Remove every installed collection.
#[tauri::command]
pub async fn icon_packs_clear(state: State<'_, Shared>) -> CmdResult<PackList> {
    let st = state.inner().clone();
    blocking(move || {
        packs::clear(&st.paths.icon_packs);
        Ok(list(&st))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The icons shown, in the same form as `icon_pack_read`.
    pub pack: String,
    /// How many icons match in the whole collection.
    pub total: usize,
}

#[cfg(desktop)]
mod net {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    fn agent() -> &'static ureq::Agent {
        static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
        AGENT.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(6)))
                .timeout_global(Some(Duration::from_secs(120)))
                .timeout_recv_body(Some(Duration::from_secs(20)))
                .user_agent("FAST-File-Explorer-icons")
                .build()
                .into()
        })
    }

    fn offline<E: ToString>(e: E) -> String {
        format!("Could not reach the internet to get the icons ({}).", e.to_string())
    }

    fn get_text(url: &str, limit: u64) -> Result<String, String> {
        let mut resp = agent().get(url).call().map_err(offline)?;
        resp.body_mut().with_config().limit(limit).read_to_string().map_err(offline)
    }

    /// The names in a collection, kept for the run so typing in the search box does not ask again.
    fn names(id: &str) -> Result<Vec<String>, String> {
        static KEPT: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
        let kept = KEPT.get_or_init(Default::default);
        if let Some(n) = kept.lock().unwrap().get(id) {
            return Ok(n.clone());
        }
        let text = get_text(&format!("https://api.iconify.design/collection?prefix={id}"), 4 * 1024 * 1024)?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "The icon list was not in the expected form.".to_string())?;
        let n = packs::names_in_listing(&v);
        if n.is_empty() {
            return Err("The icon list was empty.".into());
        }
        kept.lock().unwrap().insert(id.to_string(), n.clone());
        Ok(n)
    }

    /// Up to `count` icons of a collection matching every word of `query`, fetched just for showing.
    pub fn preview(id: &str, query: &str, count: usize) -> Result<Preview, String> {
        packs::info(id).ok_or_else(|| "Unknown icon collection.".to_string())?;
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
        let hits: Vec<String> = names(id)?.into_iter().filter(|n| words.iter().all(|w| n.contains(w.as_str()))).collect();
        let total = hits.len();
        let shown: Vec<&str> = hits.iter().take(count.clamp(1, 120)).map(|s| s.as_str()).collect();
        if shown.is_empty() {
            return Ok(Preview { pack: r#"{"w":16,"h":16,"icons":{}}"#.into(), total: 0 });
        }
        let text = get_text(&format!("https://api.iconify.design/{id}.json?icons={}", shown.join(",")), 2 * 1024 * 1024)?;
        // Same reduction as an install, so a preview can only show what could be installed.
        let (pack, _) = packs::reduce(&text).map_err(|e| e.to_string())?;
        Ok(Preview { pack, total })
    }

    /// How far a download is, for the progress bar.
    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    struct Progress<'a> {
        id: &'a str,
        /// "connecting", "downloading" or "saving".
        phase: &'a str,
        done: u64,
        /// What the whole download is expected to be (the catalog's figure), so a bar can be drawn.
        total: u64,
    }

    /// How long the first address gets before the second one is tried alongside it.
    const HEAD_START: Duration = Duration::from_millis(2500);

    /// Fetch `url` into memory, reporting the bytes so far. Gives up quietly once another address has won.
    fn fetch(url: &str, limit: u64, won: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let mut resp = agent().get(url).call().map_err(offline)?;
        let mut reader = resp.body_mut().with_config().limit(limit).reader();
        let mut out = Vec::with_capacity(512 * 1024);
        let mut buf = [0u8; 32 * 1024];
        loop {
            if won.load(Ordering::Relaxed) {
                return Err("another address was faster".into());
            }
            let n = reader.read(&mut buf).map_err(offline)?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&buf[..n]);
            progress(out.len() as u64);
        }
    }

    /// Download a whole collection, reduce it and keep it. The two addresses race: the second starts if the first
    /// has not finished (or has failed) shortly after, and whichever completes first is used.
    pub fn install(app: &tauri::AppHandle, st: &Shared, id: &str) -> Result<(), String> {
        use tauri::Emitter;
        let info = packs::info(id).ok_or_else(|| "Unknown icon collection.".to_string())?;
        let total = info.download;
        let send = {
            let app = app.clone();
            let id = id.to_string();
            move |phase: &str, done: u64| {
                let _ = app.emit("icon-pack-progress", Progress { id: &id, phase, done, total });
            }
        };
        send("connecting", 0);
        let urls = [format!("https://cdn.jsdelivr.net/npm/@iconify-json/{id}/icons.json"), format!("https://unpkg.com/@iconify-json/{id}/icons.json")];
        let won = Arc::new(AtomicBool::new(false));
        let first_failed = Arc::new(AtomicBool::new(false));
        let best = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
        for (i, url) in urls.into_iter().enumerate() {
            let (won, first_failed, best, tx) = (won.clone(), first_failed.clone(), best.clone(), tx.clone());
            let send = send.clone();
            std::thread::spawn(move || {
                if i > 0 {
                    let from = Instant::now();
                    while from.elapsed() < HEAD_START && !won.load(Ordering::Relaxed) && !first_failed.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    if won.load(Ordering::Relaxed) {
                        return;
                    }
                }
                let mut last = Instant::now();
                let r = fetch(&url, packs::MAX_DOWNLOAD, &won, &mut |n| {
                    // The bar follows whichever address is furthest, and only moves forward.
                    let top = best.fetch_max(n, Ordering::Relaxed).max(n);
                    if n >= top && last.elapsed() >= Duration::from_millis(60) {
                        last = Instant::now();
                        send("downloading", n.min(total.saturating_sub(1).max(1)));
                    }
                });
                if r.is_err() && i == 0 {
                    first_failed.store(true, Ordering::Relaxed);
                }
                let _ = tx.send(r);
            });
        }
        drop(tx);
        let mut last_err = String::from("No answer from the internet.");
        while let Ok(r) = rx.recv() {
            match r {
                Ok(bytes) => {
                    won.store(true, Ordering::Relaxed);
                    send("saving", total);
                    let text = String::from_utf8(bytes).map_err(|_| "The icon file was not text.".to_string())?;
                    let (reduced, _) = packs::reduce(&text).map_err(|e| e.to_string())?;
                    return packs::store(&st.paths.icon_packs, id, &reduced).map_err(|e| e.to_string());
                }
                Err(e) if e.contains("another address") => {}
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }
}

#[cfg(not(desktop))]
mod net {
    use super::*;
    const NO: &str = "Downloading icon collections is only available on computers.";
    pub fn preview(_: &str, _: &str, _: usize) -> Result<Preview, String> {
        Err(NO.into())
    }
    pub fn install(_: &tauri::AppHandle, _: &Shared, _: &str) -> Result<(), String> {
        Err(NO.into())
    }
}

/// A look at a collection from the internet, without installing it.
#[tauri::command]
pub async fn icon_pack_preview(id: String, query: String, count: usize) -> CmdResult<Preview> {
    blocking(move || net::preview(&id, &query, count)).await
}

/// Download and keep a collection.
#[tauri::command]
pub async fn icon_pack_install(app: tauri::AppHandle, state: State<'_, Shared>, id: String) -> CmdResult<PackList> {
    let st = state.inner().clone();
    blocking(move || {
        net::install(&app, &st, &id)?;
        Ok(list(&st))
    })
    .await
}
