//! Tauri shell: wires the UI-agnostic `fast-core` engine to the webview.

mod commands;
mod crash;
mod launch;
mod native_dnd;
mod remote;
mod smartclear;
mod state;
#[cfg(desktop)]
mod terminal;
mod tasks;
mod tools;
mod update;

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
    #[cfg_attr(mobile, allow(unused_mut))]
    let mut builder = tauri::Builder::default();
    // Starting the app again (or opening a folder with it) goes to the copy
    // that is already running. This has to be the first plugin.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let launches = fast_core::filemanager::parse_launch(&args, std::path::Path::new(&cwd));
            launch::deliver(app, launches);
        }));
        launch::queue_own_arguments();
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config = app.path().app_config_dir()?;
            let cache = app.path().app_cache_dir()?;
            let paths = Paths::new(&config, &cache);
            // Crash logging first, so even a failure while starting is written down.
            let webview = tauri::webview_version().unwrap_or_default();
            fast_core::crashlog::install_panic_hook();
            let earlier = fast_core::crashlog::init(&paths.crash, env!("CARGO_PKG_VERSION"), &webview);
            fast_core::crashlog::start_flusher();
            fast_core::crashlog::install_native_handler();
            fast_core::crashlog::start_watchdog();
            // Nothing can be reading staged files yet, so leftovers from the last run go.
            for prefix in ["paste-", "clip-", "stage-"] {
                remote::sweep_staging(&paths.remote_tmp, prefix, None);
            }

            // Only small JSON files are read before the window appears; the
            // index and drive list load on background threads.
            let settings = Settings::load(&paths.settings);
            let session = Session::load(&paths.session);
            let budget = resources::compute(&settings.performance, &resources::snapshot());

            let state = Arc::new(AppState {
                listings: Mutex::new(ListingCache::new(budget.listing_cache_dirs)),
                started: std::time::Instant::now(),
                #[cfg(desktop)]
                terminal: Default::default(),
                thumb_permits: Arc::new(tokio::sync::Semaphore::new(budget.thumb_threads.max(1))),
                slow_thumb_permits: Arc::new(tokio::sync::Semaphore::new(budget.thumb_threads.clamp(2, 6))),
                prewarm_generation: Default::default(),
                drives: RwLock::new(session.drives.clone()),
                devices: RwLock::new(Vec::new()),
                shares: Default::default(),
                remotes: Mutex::new(HashMap::new()),
                watcher: {
                    let handle = app.handle().clone();
                    fast_core::watch::Watcher::new(move |dirs| {
                        let st = handle.state::<state::Shared>();
                        commands::fs_changed(&handle, &st, dirs);
                    })
                },
                watch_sets: Mutex::new(HashMap::new()),
                predictor: Mutex::new(Predictor::load(&paths.predictor)),
                history: Mutex::new(History::load(&paths.history)),
                deletes: Mutex::new(HashMap::new()),
                job_asks: Mutex::new(HashMap::new()),
                elevated: Mutex::new(HashMap::new()),
                analyses: Mutex::new(HashMap::new()),
                settings: RwLock::new(settings),
                session: Mutex::new(session),
                budget: RwLock::new(budget),
                attention: std::sync::atomic::AtomicU8::new(0),
                index: RwLock::new(Arc::new(fast_core::indexset::IndexSet::default())),
                index_status: Mutex::new(IndexStatus { state: "idle".into(), ..Default::default() }),
                index_busy: AtomicBool::new(false),
                index_pending: Mutex::new(Default::default()),
                index_wake: Default::default(),
                search_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                size_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
                // Read in the background (a long-used cache is tens of MB of JSON): sizes show as they are found until it is in.
                folder_sizes: Mutex::new(fast_core::sizecache::SizeCache::loading()),
                locks: RwLock::new(fast_core::locks::Locks::load(&paths.locks)),
                monitor: Mutex::new(sysinfo::System::new()),
                transfers: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                visits_since_save: AtomicU64::new(0),
                paths,
            });
            app.manage(state.clone());
            {
                let st = state.clone();
                let _ = std::thread::Builder::new().name("size-cache".into()).spawn(move || {
                    let loaded = fast_core::sizecache::SizeCache::load(&st.paths.sizes);
                    st.folder_sizes.lock().finish_loading(loaded);
                });
            }
            crash::remember_previous(earlier);
            crash::send_pending(&state);

            #[cfg(desktop)]
            launch::start(app.handle());
            commands::start_indexing(app.handle().clone(), state.clone());
            commands::start_drive_watch(app.handle().clone(), state.clone());
            commands::apply_transfer_limits(&state);
            update::start(app.handle(), state.clone());
            tasks::start(app.handle(), state.clone());
            // Started with the computer: stay out of the way until the person opens it.
            #[cfg(desktop)]
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.minimize();
                }
            }

            // Re-evaluate the resource budget periodically: plugging in or
            // unplugging a laptop changes what background work is allowed.
            let handle = app.handle().clone();
            std::thread::Builder::new().name("governor".into()).spawn(move || {
                let mut tick: u64 = 0;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    commands::apply_budget(&handle, &state);
                    // Every ten minutes, keep the thumbnail cache inside its size limit.
                    if tick % 20 == 1 {
                        commands::trim_thumbnails(&state);
                    }
                    tick += 1;
                }
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
            if matches!(event, tauri::WindowEvent::Destroyed) {
                // A closed window no longer needs its folders watched.
                let st = window.state::<state::Shared>();
                let all: Vec<std::path::PathBuf> = {
                    let mut sets = st.watch_sets.lock();
                    sets.remove(window.label());
                    sets.values().flatten().cloned().collect()
                };
                st.watcher.set_dirs(all);
            }
            #[cfg(desktop)]
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Focused(_) | tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)) {
                commands::attention_changed(window.app_handle());
            }
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let st = window.state::<state::Shared>();
                let _ = st.predictor.lock().save(&st.paths.predictor);
                let _ = st.session.lock().save(&st.paths.session);
            }
        })
        .invoke_handler(tauri::generate_handler![
            tools::tool_run,
            tools::tool_cancel,
            tools::view_info,
            tools::view_read,
            tools::view_find,
            commands::thumbs_prewarm,
            commands::stat_many,
            commands::trash_list,
            commands::trash_restore,
            commands::trash_purge,
            commands::trash_empty,
            commands::rename_by_copy,
            commands::folder_fill,
            commands::run_program,
            tools::preview_external,
            tasks::tasks_list,
            tasks::tasks_save,
            tasks::tasks_delete,
            tasks::tasks_run,
            tasks::tasks_cancel,
            tasks::tasks_preview,
            tasks::tasks_history,
            tasks::tasks_stats,
            tasks::tasks_clear_history,
            tasks::tasks_export,
            tasks::tasks_import,
            tasks::autostart_get,
            tasks::autostart_set,
            commands::smartctl_installed,
            commands::install_smartmontools,
            commands::list_dir,
            commands::list_dir_cancel,
            smartclear::smart_clear_scan,
            smartclear::smart_clear_cancel,
            launch::take_launch,
            launch::default_fm_status,
            launch::default_fm_set,
            remote::remote_connect,
            remote::remote_disconnect,
            remote::remote_is_connected,
            remote::remote_mkdir,
            remote::remote_rename,
            remote::remote_copy_start,
            remote::archive_copy_start,
            remote::archive_paste_start,
            remote::archive_delete,
            remote::archive_rename,
            remote::archive_create,
            remote::archive_stage,
            remote::remote_remove_start,
            remote::servers_list,
            remote::servers_save,
            remote::servers_remove,
            remote::servers_forget_host,
            commands::prefetch,
            commands::record_visit,
            commands::frequent_folders,
            commands::get_details,
            commands::folder_stats,
            commands::open_window,
            commands::drop_tab,
            commands::tab_hover,
            native_dnd::clipboard_set_files,
            native_dnd::clipboard_get_files,
            native_dnd::clipboard_get,
            native_dnd::clipboard_clear,
            native_dnd::clipboard_stage_virtual,
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
            commands::read_for_view,
            commands::get_drives,
            commands::get_devices,
            commands::network_link,
            commands::connect_device,
            commands::open_device_in_system,
            commands::known_folders,
            commands::index_status,
            commands::reindex,
            commands::search,
            commands::analysis_snapshot,
            commands::analyze_start,
            commands::analyze_cancel,
            commands::check_conflicts,
            commands::watch_dirs,
            commands::archive_start,
            commands::trash_paths,
            commands::restore_from_trash,
            commands::transfer_start,
            commands::transfer_control,
            commands::transfer_adjust,
            update::update_check,
            update::update_status,
            update::update_download,
            update::update_install,
            update::update_discard,
            commands::transfer_force_stop,
            commands::transfer_resolve,
            commands::check_access,
            commands::elevated_start,
            #[cfg(desktop)]
            terminal::term_info,
            #[cfg(desktop)]
            terminal::term_list,
            #[cfg(desktop)]
            terminal::term_spawn,
            #[cfg(desktop)]
            terminal::term_attach,
            #[cfg(desktop)]
            terminal::term_input,
            #[cfg(desktop)]
            terminal::term_resize,
            #[cfg(desktop)]
            terminal::term_do,
            #[cfg(desktop)]
            terminal::term_popout,
            #[cfg(desktop)]
            terminal::term_open_url,
            #[cfg(desktop)]
            terminal::term_transcript,
            #[cfg(desktop)]
            terminal::term_sweep_transcripts,
            commands::delete_start,
            commands::delete_control,
            commands::show_properties,
            commands::history_info,
            commands::transfer_methods,
            commands::column_values,
            commands::clear_history,
            commands::rename_item,
            commands::create_item,
            commands::open_item,
            commands::open_choices,
            commands::open_with_app,
            commands::open_system_chooser,
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
            commands::drive_eject,
            commands::locks_list,
            commands::lock_add,
            commands::lock_unlock,
            commands::lock_remove,
            commands::lock_set_main,
            commands::locks_relock,
            commands::resource_usage,
            commands::compact_memory,
            commands::disk_smart_admin,
            commands::clear_cache,
            commands::clear_cache_for,
            crash::crash_previous,
            crash::crash_heartbeat,
            crash::crash_folder,
            crash::crash_open_folder,
            crash::crash_note,
            crash::crash_context,
            crash::crash_report,
            crash::crash_logs,
            crash::crash_log_read,
            crash::crash_send,
            crash::crash_log_save,
            crash::save_text_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building FAST File Explorer")
        .run(|app, event| {
            // Links only work while the app runs: end them (and any tunnel process) on the way out.
            // Stop what the tasks are doing (and note how far they got) before the app goes.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                tasks::shutdown(app);
            }
            if let tauri::RunEvent::Exit = event {
                fast_core::crashlog::note("app closing");
                fast_core::crashlog::clean_exit();
                tasks::shutdown(app);
                #[cfg(desktop)]
                terminal::on_exit(app);
                use tauri::Manager;
                if let Some(state) = app.try_state::<state::Shared>() {
                    state.shares.stop_all();
                }
            }
            // macOS: a folder or file handed to the app by the system (Open With,
            // the Dock, or another app revealing a file). iOS: a file sent with "Open in".
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls } = &event {
                let found = urls
                    .iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .filter_map(|p| fast_core::filemanager::launch_for(&p, false))
                    .collect();
                launch::deliver(app, fast_core::filemanager::merge(found));
            }
        });
}

/// Headless mode used for operations run through the OS's administrator
/// prompt: do one job and exit. Returns the process exit code.
pub fn run_helper(args: &[String]) -> i32 {
    // The terminal host (keeps the shells) and a window attached to one of them.
    #[cfg(desktop)]
    if let Some(i) = args.iter().position(|a| a == "--terminal-host") {
        return fast_core::terminal::host::serve(std::path::Path::new(args.get(i + 1).map(String::as_str).unwrap_or(".")));
    }
    #[cfg(desktop)]
    if let Some(i) = args.iter().position(|a| a == "--terminal-attach") {
        return match (args.get(i + 1), args.get(i + 2)) {
            (Some(dir), Some(sid)) => fast_core::terminal::attach::run(std::path::Path::new(dir), sid),
            _ => 2,
        };
    }
    // What the uninstaller runs: stop being the system's file manager.
    #[cfg(desktop)]
    if args.iter().any(|a| a == "--restore-file-manager") {
        return match fast_core::filemanager::restore_for("dev.envid.fastfileexplorer") {
            Ok(()) => 0,
            Err(_) => 1,
        };
    }
    // An automated task that needs administrator rights.
    if args.iter().any(|a| a == "--run-task") {
        return fast_core::tasks::helper_main(args);
    }
    fast_core::ops::helper_main(args)
}
