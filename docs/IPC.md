# Command reference (IPC)

Every call the interface makes into the Rust shell. Generated from `src-tauri/src/commands.rs`; the typed wrapper for each is in `src/api.ts` under the camelCase name. Arguments are sent as an object with camelCase keys. All commands return a promise; a failure rejects with a message string.

| Command | `api.` wrapper | Arguments | Returns | Notes |
|---|---|---|---|---|
| `list_dir` | `listDir` | `path: String` | `Listing` |  |
| `prefetch` | `prefetch` | `paths: Vec<String>` | `()` | Warm the listing cache for folders the user is likely to open next. |
| `record_visit` | `recordVisit` | `from: Option<String>, to: String` | `Vec<String>` | Record a navigation and return the folders most likely to be opened next. Their listings are warmed in the background. |
| `frequent_folders` | `frequentFolders` | `limit: usize` | `Vec<String>` | The folders opened most often (recent visits weigh more), skipping any that no longer exist. |
| `get_details` | `getDetails` | `path: String` | `Details` |  |
| `folder_stats` | `folderStats` | `path: String` | `FolderStats` |  |
| `folder_sizes_start` | `folderSizesStart` | `dir: String, names: Vec<String>, refresh: bool` | `()` | Measure the given subfolders of `dir` one after another in the background, sending a `folder-size` event for each. Starting another job (opening a different folder) cancels this one. Recent results are reused unless `refresh` is set. |
| `hash_file` | `hashFile` | `path: String` | `String` |  |
| `thumbnail` | `thumbnail` | `path: String, size: u32` | `tauri::ipc::Response` |  |
| `get_drives` | `getDrives` | `—` | `Vec<Drive>` | Query the OS for volumes. The frontend shows the cached list from the session first, so this never delays first paint. |
| `known_folders` | `knownFolders` | `—` | `Vec<Place>` |  |
| `index_status` | `indexStatus` | `—` | `IndexStatus` |  |
| `reindex` | `reindex` | `—` | `()` |  |
| `search` | `search` | `query: String, scope: Option<String>, id: u64` | `SearchResponse` | Search. Whatever the index covers is answered at once, from memory. Any folder it does not cover yet (a drive still being indexed, a folder made a moment ago) is walked live on another thread, and its matches are sent as `search-hits` events as they are found, so nothing waits for a full scan. |
| `analysis_snapshot` | `analysisSnapshot` | `path: String` | `Option<AnalysisResult>` | The last saved result for a folder, if any: tiny to read, so the view can show it immediately while a fresh scan runs. |
| `analyze_start` | `analyzeStart` | `path: String` | `()` | Scan a folder in the background. Progress arrives as `analysis-progress` events and the finished result as `analysis-done` (and is saved as the new snapshot). Starting a scan of a path that is already being scanned replaces the old scan. |
| `analyze_cancel` | `analyzeCancel` | `path: String` | `()` |  |
| `check_conflicts` | `checkConflicts` | `sources: Vec<String>, dest_dir: String` | `Vec<String>` | Names in `dest_dir` that the given sources would collide with. |
| `watch_dirs` | `watchDirs` | `dirs: Vec<String>` | `()` | The folders this window has open. They are watched (not recursively); a change made by another program invalidates the cached listing and emits `fs-changed`. An empty list stops watching for this window. |
| `trash_paths` | `trashPaths` | `paths: Vec<String>` | `()` | Move items to the Trash right away and wait for it (Undo uses this; bigger deletes go through `delete_start`). |
| `restore_from_trash` | `restoreFromTrash` | `paths: Vec<String>` | `usize` | Put items back from the Trash; returns how many were found there. Linux and Windows only. |
| `archive_start` | `archiveStart` | `action: String, paths: Vec<String>, here: bool` | `u64` | Make a zip next to the first item (`action` "compress"), or unpack any readable archive (zip, tar family, 7z, gz/bz2/xz, and rar, cab, iso and others when 7-Zip or bsdtar is installed) into a folder named after the archive or straight into its folder ("extract"). Runs in the background and reports on the transfers card through `delete-progress` (with `action` and `result` set); `delete_control` cancels it and removes a half-written zip. Symbolic links are left out of zips; entries that would leave the folder are refused; existing files are kept. |
| `archive_copy_start` | `archiveCopyStart` | `sources: Vec<String>, dest: String, open: bool` | `u64` | Unpack items from inside archives (locations like `archive:///home/me/a.zip!/docs`, see `fast_core::browse`) into the folder `dest`, or with `open` one file into a temporary folder (`result` names it, and the interface opens it). Reports through `delete-progress` with `action` "unpack" or "open". Files already in `dest` are kept. `list_dir` and `get_details` also accept these locations; everything inside an archive is read-only. |
| `archive_paste_start` / `archive_delete` / `archive_rename` / `archive_create` / `archive_stage` | `archivePasteStart` etc. | see `src/api.ts` | job id, or none | Change archives (zip, tar, tar.gz, 7z; see `fast_core::edit`): paste from disk or another archive into one (`op` "copy" or "move"; moves inside the same archive just rename entries), delete, rename, make a folder or file, and `archive_stage` unpacks items for the system clipboard. `archive_copy_start` also takes `remove` to take items out of the archive after they have been copied. Reports as action "pack". |
| `remote_connect` | `remoteConnect` | `server: Server, path: Option<String>` | `String` | Log in to an FTP, SFTP or WebDAV server and return the address of `path` (or the starting folder). The password stays in memory. An SFTP server whose key is new or changed fails with `untrusted-host:<fingerprint>` or `changed-host:<fingerprint>`; connect again with `server.trust` set to that fingerprint to accept it. |
| `remote_disconnect` | `remoteDisconnect` | `location: String` | `()` | Forget a server's login and close its connections. |
| `remote_is_connected` | `remoteIsConnected` | `location: String` | `bool` | Whether a login for this server is held in memory. |
| `remote_mkdir` | `remoteMkdir` | `parent: String, name: String` | `String` | Make a folder on a server; returns its address. |
| `remote_rename` | `remoteRename` | `from: String, new_name: String` | `String` | Rename on a server; returns the new address. |
| `remote_copy_start` | `remoteCopyStart` | `sources: Vec<String>, dest: String, mv: bool, open: bool` | `u64` | Upload, download, or copy between servers, as a background job reporting through `delete-progress` (`action` "upload", "download" or "open"; `bytes_done`/`bytes_total` set). Name clashes get "name (2)". With `mv`, originals are deleted only after their copy fully succeeded; inside one server it is a rename. With `open`, one file is fetched to a temporary place and the finished event's `result` is its path. |
| `remote_remove_start` | `remoteRemoveStart` | `paths: Vec<String>` | `u64` | Delete files and folders on a server (no trash). Progress as `action` "remote-delete". |
| `servers_list` / `servers_save` / `servers_remove` | `serversList` / `serversSave` / `serversRemove` | | `Vec<SavedServer>` | The server addresses kept in the sidebar (never passwords). |
| `servers_forget_host` | `serversForgetHost` | `host: String, port: u16` | `()` | Drop a remembered SFTP fingerprint. |
| `transfer_start` | `transferStart` | `mut spec: TransferSpec` | `u64` |  |
| `transfer_control` | `transferControl` | `id: u64, action: String` | `()` |  |
| `transfer_resolve` | `transferResolve` | `id: u64, answer: String, all: bool` | `()` | Answer the "file already exists" question a running transfer is showing. `answer` is `skip`, `replace` or `keepBoth`; with `all`, it applies to every later conflict in the same transfer. |
| `transfer_force_stop` | `transferForceStop` | `id: u64` | `()` | Stop a transfer without waiting for it to answer. A normal cancel is a request the copy has to notice, and one stuck waiting on a dead network drive never does. This asks it to stop, stops any copy tool outright, and lets go of the job at once; whatever it is stuck in is abandoned and ends whenever the drive finally answers (or when the app closes). |
| `check_access` | `checkAccess` | `job: Job` | `AccessCheck` | Would the current user be refused if this job ran now? The app calls this *before* running anything and asks the person first if the answer is yes. |
| `elevated_start` | `elevatedStart` | `job: Job` | `u64` | Run a job through the OS administrator prompt. Returns at once with the job's id; progress arrives as `transfer-progress` / `delete-progress` events and the outcome as a `job-finished` event. If the person dismisses the prompt, nothing runs and `job-finished` says `declined`. |
| `delete_start` | `deleteStart` | `paths: Vec<String>, permanent: bool` | `u64` | Delete in the background with progress (`delete-progress` events). Returns the job id. |
| `delete_control` | `deleteControl` | `id: u64, action: String` | `()` |  |
| `show_properties` | `showProperties` | `paths: Vec<String>` | `bool` | Show the operating system's own properties window. Returns false when it has none for this request (or the person turned that off in settings), so the app shows its own. |
| `history_info` | `historyInfo` | `—` | `HistoryInfo` |  |
| `clear_history` | `clearHistory` | `—` | `()` |  |
| `rename_item` | `renameItem` | `path: String, new_name: String` | `String` |  |
| `create_item` | `createItem` | `dir: String, name: String, folder: bool` | `String` |  |
| `open_item` | `openItem` | `path: String` | `()` |  |
| `open_terminal` | `openTerminal` | `path: String, mode: TerminalMode` | `()` |  |
| `get_settings` | `getSettings` | `—` | `Settings` |  |
| `set_settings` | `setSettings` | `settings: Settings` | `()` |  |
| `get_session` | `getSession` | `—` | `Session` |  |
| `save_session` | `saveSession` | `mut session: Session` | `()` |  |
| `app_info` | `appInfo` | `—` | `AppInfo` |  |
| `system_report` | `systemReport` | `refresh: Option<bool>` | `FullReport` | Everything the About page shows. Reads hardware and drive health off the UI thread; a saved copy (`hardware-report.json`, Settings > Storage) is used while it is recent enough, and `refresh` forces a new reading. |
| `cache_usage` | `cacheUsage` | `—` | `Vec<CacheEntry>` | How much storage every kind of cache uses, so each can be cleared alone. |
| `clear_cache` | `clearCache` | `id: String` | `()` | Clear one kind of cache (ids from `cache_usage`), or "all". |
| `network_map` | `networkMap` | `remote: String, letter: Option<String>` | `Option<String>` | Connect a network share. Windows maps it to a drive letter (`letter` like "Z:", or a free one when empty); macOS mounts it under /Volumes; Linux mounts it through the desktop's GVFS. Returns a folder to open, if known. |
| `network_disconnect` | `networkDisconnect` | `mount: String` | `()` | Disconnect a network drive (`mount` like "Z:\\" on Windows, a mount point elsewhere). |
| `shell_menu` | `shellMenu` | `paths: Vec<String>, background: bool` | `Vec<fast_core::shellmenu::ShellItem>` | Entries other programs add to the right-click menu for these items (or for a folder's background). Slow, so the app shows its own menu first. |
| `shell_invoke` | `shellInvoke` | `id: u32` | `()` |  |
| `create_shortcut` | `createShortcut` | `path: String` | `String` | Make a shortcut next to an item: a symbolic link on Linux and macOS, a `.lnk` file on Windows. |
| `import_asset` | `importAsset` | `kind: String, path: String` | `String` | Copy a file the person chose into the app's own folder; returns its name. |
| `list_assets` | `listAssets` | `kind: String` | `Vec<Asset>` |  |
| `read_asset` | `readAsset` | `kind: String, name: String` | `tauri::ipc::Response` |  |
| `delete_asset` | `deleteAsset` | `kind: String, name: String` | `()` |  |
| `run_custom_action` | `runCustomAction` | `id: String, paths: Vec<String>` | `()` | Run one of the person's own actions. The program is started directly, never through a shell, so file names cannot inject commands. |
| `config_folder` | `configFolder` | `—` | `Option<String>` | The folder that holds the app's settings, sessions and imported files. |
| `import_settings` | `importSettings` | `json: String` | `Settings` | Replace all settings with a JSON document (import, or the raw editor). Unknown keys are dropped and missing ones take their defaults. |
| `open_window` | `openWindow` | `location: String, x: Option<f64>, y: Option<f64>` | `()` | Open another window showing `location` in its only tab, near the given screen position. Used when a tab is dragged out of the tab bar. |
| `drop_tab` | `dropTab` | `location: String, x: f64, y: f64, close_source: bool` | `bool` | A tab was let go at screen position (x, y) of the calling window. If that is over the tab bar of another app window, hand the tab to it (it picks the slot from the position) and report true; the caller then removes its copy. With `close_source` the calling window had only that tab, so it closes. |
| `open_transfers_window` | `openTransfersWindow` | `—` | `()` |  |
| `open_settings_window` | `openSettingsWindow` | `—` | `()` |  |
| `reattach` | `reattach` | `kind: String, payload: Option<serde_json::Value>` | `()` | Put a detached panel back into the main window: tell the main window what came back, bring it forward, and close the window this was called from. |
| `resolve_path` | `resolvePath` | `input: String` | `ResolvedPath` | Turn what someone typed in the address bar into a real path: quotes, `~`, `%VAR%` / `$VAR`, `/` on Windows and a bare drive letter are all understood. Fails with a readable message when nothing is there. |
| `share_start` | `shareStart` | `path: String, minutes: u64, max_downloads: u32, lan: bool, internet: bool` | `fast_core::share::ShareInfo` | Start serving one file at a secret, temporary address. |
| `share_internet_available` | `shareInternetAvailable` | — | `bool` | Is `cloudflared` installed (needed for internet shares)? |
| `share_stop` | `shareStop` | `id: String` | `()` |  |
| `share_list` | `shareList` | `—` | `Vec<fast_core::share::ShareInfo>` |  |
| `open_choices` | `openChoices` | `path: String` | `fast_core::openwith::Choices` | The apps that can open a file: the default, suggested ones and the rest. |
| `open_with_app` | `openWithApp` | `path: String, app: String, always: bool` | `()` | Open a file with a chosen app; `always` also makes it the default for the file type. |
| `open_system_chooser` | `openSystemChooser` | `path: String` | `()` | Windows' own "Open with" chooser. Other systems use the app's own. |
| `drive_eject` | `driveEject` | `mount: String` | `()` | Safely remove a detachable drive. |
| `tab_hover` | `tabHover` | `x: f64, y: f64, width: f64, active: bool` | `()` | While a tab is dragged outside its window: tell the window under the pointer to outline where it would land (event `tab-hover`). |
| `locks_list` | `locksList` | — | `LocksState` | Which folders and drives are locked, and which are open now. Hashes never leave the backend. |
| `lock_add` | `lockAdd` | `path: String, passcode: String, main_password: Option<String>` | `LocksState` | Lock a folder or drive; the first lock may also set the main password. |
| `lock_unlock` | `lockUnlock` | `path: String, passcode: String` | `bool` | Try a passcode for whatever covers `path`. |
| `lock_remove` | `lockRemove` | `id: String, secret: String` | `LocksState` | Remove a lock; needs the main password (or the lock's own passcode when there is no main password). |
| `lock_set_main` | `lockSetMain` | `old: String, new: String` | `LocksState` | Set or change the main password. |
| `locks_relock` | `locksRelock` | — | `LocksState` | Close every lock again. |
| `resource_usage` | `resourceUsage` | — | `ResourceUsage` | The app's own processor, memory and disk use, plus what it keeps in caches. |
| `compact_memory` | `compactMemory` | — | `(u64, u64)` | Drop cached listings and return free memory to the system. Returns memory before and after. |
| `disk_smart_admin` | `diskSmartAdmin` | `device: String` | `FullReport` | Read one drive's SMART data with administrator rights (through the OS prompt) and update the saved hardware report. |

## Events

Sent from the shell to the interface (`listen()` in `src/api.ts` / `App.tsx`).

| Event | Meaning |
|---|---|
| `transfer-progress` | A transfer job's `Progress` snapshot (rate set by `transfer.progressIntervalMs` or the resource budget) |
| `delete-progress` | Progress of a delete job |
| `job-finished` | A transfer or delete ended (done, cancelled or failed) |
| `dir-updated` | A cached folder listing changed after a background re-read |
| `fs-changed` | The watcher saw changes in a watched folder |
| `folder-size` | One folder's computed size arrived |
| `search-hits` | A batch of search results |
| `index-status` | Indexing progress for a volume |
| `budget-changed` | The resource budget was re-evaluated |
| `settings-changed` | Settings were saved in some window; every window reloads them |
| `detached` | A settings/transfers window opened or closed |
| `analysis-partial`, `analysis-done`, `analysis-failed` | Disk usage map progress |
| `tab-drop` | A tab was dropped on this window's tab bar (carries the tab and the screen position) |
| `tab-hover` | Another window is dragging a tab over this window's tab bar (the slot to outline), or has left it |
| `reattach` | A detached window came back to the main window |
