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
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    fn agent() -> &'static ureq::Agent {
        static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
        AGENT.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(6)))
                .timeout_global(Some(Duration::from_secs(90)))
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

    /// Download a whole collection, reduce it and keep it.
    pub fn install(st: &Shared, id: &str) -> Result<(), String> {
        packs::info(id).ok_or_else(|| "Unknown icon collection.".to_string())?;
        let mut last = String::new();
        for base in ["https://cdn.jsdelivr.net/npm/@iconify-json", "https://unpkg.com/@iconify-json"] {
            match get_text(&format!("{base}/{id}/icons.json"), packs::MAX_DOWNLOAD) {
                Ok(text) => {
                    let (reduced, _) = packs::reduce(&text).map_err(|e| e.to_string())?;
                    return packs::store(&st.paths.icon_packs, id, &reduced).map_err(|e| e.to_string());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

#[cfg(not(desktop))]
mod net {
    use super::*;
    const NO: &str = "Downloading icon collections is only available on computers.";
    pub fn preview(_: &str, _: &str, _: usize) -> Result<Preview, String> {
        Err(NO.into())
    }
    pub fn install(_: &Shared, _: &str) -> Result<(), String> {
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
pub async fn icon_pack_install(state: State<'_, Shared>, id: String) -> CmdResult<PackList> {
    let st = state.inner().clone();
    blocking(move || {
        net::install(&st, &id)?;
        Ok(list(&st))
    })
    .await
}
