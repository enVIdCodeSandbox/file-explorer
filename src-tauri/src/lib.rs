//! Tauri shell: wires the UI-agnostic `fast-core` engine to the webview.

mod commands;
mod native_dnd;
mod state;

use fast_core::history::History;
use fast_core::predict::Predictor;
use fast_core::resources;
use fast_core::session::Session;
use fast_core::settings::Settings;
use parking_lot::{Mutex, RwLock};
use state::{AppState, IndexStatus, ListingCache, Paths};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config = app.path().app_config_dir()?;
            let cache = app.path().app_cache_dir()?;
            let paths = Paths::new(&config, &cache);

            // Only small JSON files are read before the window appears; the
            // index and drive list load on background threads.
            let settings = Settings::load(&paths.settings);
            let session = Session::load(&paths.session);
            let budget = resources::compute(&settings.performance, &resources::snapshot());

            let state = Arc::new(AppState {
                listings: Mutex::new(ListingCache::new(budget.listing_cache_dirs)),
                started: std::time::Instant::now(),
                thumb_permits: Arc::new(tokio::sync::Semaphore::new(budget.thumb_threads.max(1))),
                drives: RwLock::new(session.drives.clone()),
                shares: Default::default(),
                predictor: Mutex::new(Predictor::load(&paths.predictor)),
                history: Mutex::new(History::load(&paths.history)),
                deletes: Mutex::new(HashMap::new()),
                elevated: Mutex::new(HashMap::new()),
                analyses: Mutex::new(HashMap::new()),
                settings: RwLock::new(settings),
                session: Mutex::new(session),
                budget: RwLock::new(budget),
                index: RwLock::new(Arc::new(fast_core::indexset::IndexSet::default())),
                index_status: Mutex::new(IndexStatus { state: "idle".into(), ..Default::default() }),
                index_busy: AtomicBool::new(false),
                search_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                size_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                folder_sizes: Mutex::new(HashMap::new()),
                transfers: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                visits_since_save: AtomicU64::new(0),
                paths,
            });
            app.manage(state.clone());

            commands::start_indexing(app.handle().clone(), state.clone());

            // Re-evaluate the resource budget periodically: plugging in or
            // unplugging a laptop changes what background work is allowed.
            let handle = app.handle().clone();
            std::thread::Builder::new().name("governor".into()).spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(30));
                commands::apply_budget(&handle, &state);
            })?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Panels that were detached from the main window go with it.
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                for label in ["settings", "transfers"] {
                    if let Some(w) = window.app_handle().get_webview_window(label) {
                        let _ = w.destroy();
                    }
                }
            }
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let st = window.state::<state::Shared>();
                let _ = st.predictor.lock().save(&st.paths.predictor);
                let _ = st.session.lock().save(&st.paths.session);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::prefetch,
            commands::record_visit,
            commands::frequent_folders,
            commands::get_details,
            commands::folder_stats,
            commands::open_window,
            commands::drop_tab,
            native_dnd::clipboard_set_files,
            native_dnd::clipboard_get_files,
            native_dnd::drag_out,
            commands::share_start,
            commands::share_stop,
            commands::share_internet_available,
            commands::share_list,
            commands::system_report,
            commands::open_transfers_window,
            commands::open_settings_window,
            commands::reattach,
            commands::resolve_path,
            commands::folder_sizes_start,
            commands::hash_file,
            commands::thumbnail,
            commands::get_drives,
            commands::known_folders,
            commands::index_status,
            commands::reindex,
            commands::search,
            commands::analysis_snapshot,
            commands::analyze_start,
            commands::analyze_cancel,
            commands::check_conflicts,
            commands::transfer_start,
            commands::transfer_control,
            commands::transfer_force_stop,
            commands::transfer_resolve,
            commands::check_access,
            commands::elevated_start,
            commands::delete_start,
            commands::delete_control,
            commands::show_properties,
            commands::history_info,
            commands::clear_history,
            commands::rename_item,
            commands::create_item,
            commands::open_item,
            commands::open_terminal,
            commands::get_settings,
            commands::set_settings,
            commands::get_session,
            commands::save_session,
            commands::app_info,
            commands::cache_usage,
            commands::network_map,
            commands::shell_menu,
            commands::run_custom_action,
            commands::config_folder,
            commands::import_settings,
            commands::shell_invoke,
            commands::create_shortcut,
            commands::import_asset,
            commands::list_assets,
            commands::read_asset,
            commands::delete_asset,
            commands::network_disconnect,
            commands::clear_cache,
        ])
        .build(tauri::generate_context!())
        .expect("error while building FAST File Explorer")
        .run(|app, event| {
            // Links only work while the app runs: end them (and any tunnel process) on the way out.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                if let Some(state) = app.try_state::<state::Shared>() {
                    state.shares.stop_all();
                }
            }
        });
}

/// Headless mode used for operations run through the OS's administrator
/// prompt: do one job and exit. Returns the process exit code.
pub fn run_helper(args: &[String]) -> i32 {
    fast_core::ops::helper_main(args)
}
