//! User settings. Every field has a default, so adding a setting never breaks
//! an existing settings file. The frontend renders its settings screen from
//! `src/settings/schema.ts`, which must list the same keys.

use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub general: General,
    pub appearance: Appearance,
    pub performance: Performance,
    pub transfer: Transfer,
    pub updates: Updates,
    pub search: Search,
    pub context_menu: ContextMenu,
    pub terminal: Terminal,
    /// Rarely needed options that override built-in defaults, by key. The
    /// list of keys and their defaults lives in the interface
    /// (src/settings/advanced.ts); only changed values are stored.
    pub advanced: std::collections::BTreeMap<String, serde_json::Value>,
    /// Keyboard shortcuts that differ from the defaults, by command id.
    pub keys: std::collections::BTreeMap<String, String>,
    /// Programs the person added to the right-click menu.
    pub custom_actions: Vec<CustomAction>,
    /// Commands the person defined for the button bar and the Commands menu (Total Commander style: the
    /// parameters can hold %P, %N, %S… for the folder, the item under the cursor and the selection).
    pub user_commands: Vec<UserCommand>,
    /// Saved sets of settings, switchable in one click.
    pub profiles: Vec<Profile>,
    /// The id of the profile in use, if any.
    pub active_profile: String,
    /// False until the person has been through the first-run setup.
    pub setup_done: bool,
    /// What the person decided about macOS folder permissions (see `access.rs`).
    pub mac_access: MacAccess,
}

/// macOS asks permission per kind of folder unless the app has Full Disk Access. The app asks for that one permission
/// itself, once, and never nags afterwards; these are the person's choices.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MacAccess {
    /// The permission screen has been shown at least once.
    pub asked: bool,
    /// The person chose "Do not show this at startup": the screen stays away even though access is not working yet.
    /// Without that choice it comes back at every launch until access is verified; Settings > macOS permissions
    /// always has it.
    pub dismissed: bool,
    /// Full Disk Access has worked at least once, so when it stops (an update makes macOS forget it) the screen can say so.
    pub verified: bool,
    /// Kinds of folder the person allowed one by one, as macOS reported when it was asked (ids from `access::categories`).
    /// macOS applies each to everything inside the folder; the app remembers them so background work can use them.
    pub granted: Vec<String>,
    /// Who macOS thought the app was when `granted` was written (`access::identity_token`). If it differs now, macOS has
    /// forgotten those answers and they are not trusted.
    pub granted_for: String,
    /// The app's identity (`access::identity_token`) for which the stale Full Disk Access entry was already cleared once,
    /// so it is cleared automatically once per build and never in a loop.
    pub reset_for: String,
    /// Kinds of folder (ids from `access::categories`) the person does not want searched, sized or watched in the
    /// background even though the app is allowed to.
    pub off: Vec<String>,
}

/// A command for the button bar / Commands menu. The program is started directly, never through a shell.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UserCommand {
    pub id: String,
    pub label: String,
    /// One or two characters (an emoji, a letter) drawn on the button.
    pub icon: String,
    pub program: String,
    /// Parameters, with the placeholders described in the user guide.
    pub params: String,
    /// The folder to run in; empty = the folder being viewed. Placeholders work here too.
    pub start_dir: String,
    /// "program" (start it) or "terminal" (type program and parameters as one command line into a new terminal tab).
    pub run_in: String,
    pub on_button_bar: bool,
    pub in_menu: bool,
}

/// A saved set of settings. `snapshot` is a Settings object without the
/// profile fields; built-in profiles are written by the app, the rest by the
/// person (or saved automatically from their changes).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub built_in: bool,
    pub snapshot: serde_json::Value,
}

/// A user-defined right-click entry that runs a program on the selection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomAction {
    pub id: String,
    pub label: String,
    pub program: String,
    /// Arguments; `{path}` is replaced by each selected path, `{dir}` by its folder.
    pub args: String,
    /// "any", "file" or "folder"
    pub applies_to: String,
    /// Only for files with one of these extensions (comma separated, no dots).
    pub extensions: String,
    /// Run once with every selected path instead of once per path.
    pub all_at_once: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct General {
    pub show_hidden: bool,
    pub show_extensions: bool,
    /// Opening a search result opens a new tab so the results stay available.
    pub open_search_result_in_new_tab: bool,
    pub restore_session: bool,
    pub confirm_delete: bool,
    pub delete_to_trash: bool,
    /// "home" or "thisPc"
    pub start_page: String,
    pub folders_first: bool,
    /// Properties opens the operating system's own window when it has one.
    pub native_properties: bool,
    /// Add the entries other programs put in the right-click menu (Edit with…,
    /// archivers, Open with). Turn off for the fastest possible menu.
    pub shell_extras: bool,
    /// Folders that are open refresh by themselves when something else changes them.
    pub live_refresh: bool,
}

impl Default for General {
    fn default() -> Self {
        General {
            show_hidden: false,
            show_extensions: true,
            open_search_result_in_new_tab: true,
            restore_session: true,
            confirm_delete: true,
            delete_to_trash: true,
            start_page: "home".into(),
            folders_first: true,
            native_properties: true,
            shell_extras: true,
            live_refresh: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// "dark", "light" or "system"
    pub theme: String,
    pub accent: String,
    /// "compact" (Windows 10 density) or "comfortable"
    pub density: String,
    /// User CSS appended after the built-in theme. See docs/THEMING.md.
    pub custom_css: String,
    /// "bar" or "donut"
    pub drive_chart: String,
    /// "used" or "free"
    pub drive_metric: String,
    pub show_details_pane: bool,
    /// Folder view: 0 details, 1 list, 2..=5 small to extra large icons.
    /// Ctrl + scroll changes it and it is remembered.
    pub view_level: u8,
    /// Sidebar width in pixels when expanded.
    pub sidebar_width: u32,
    /// Sidebar shrunk to just its icons.
    pub sidebar_collapsed: bool,
    pub show_frequent: bool,
    pub show_pinned: bool,
    pub show_drives: bool,
    /// This PC: show how much space each drive has (on by default).
    pub show_drive_space: bool,
    /// The navigation pane's drive list: a bar under each drive showing its space (off by default). What the bar
    /// shows, used or free, follows `drive_metric`, the choice made on the This PC page.
    pub sidebar_drive_space: bool,
    /// Drives built into this computer, in the navigation pane and on This PC.
    pub show_local_drives: bool,
    /// Network drives and shares, in the navigation pane and on This PC.
    pub show_network_drives: bool,
    /// Hide the server address (like "\\\\192.168.1.5") in network drive names and paths.
    pub hide_network_address: bool,
    /// Names the person gave drives inside this app, keyed by mount point.
    pub drive_names: std::collections::BTreeMap<String, DriveName>,
    /// Layout of the window's panels: where each is docked, floating,
    /// popped out, collapsed or hidden. Opaque to the backend.
    pub layout: serde_json::Value,
    /// Look beyond the color theme: fonts, menus, icons, background, corners.
    pub style: Style,
}

/// A drive's local display settings. The name only applies while the system's
/// own name for the drive is still `base`; if the drive is renamed in the
/// system (File Explorer, Finder, ...), the system's new name wins.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DriveName {
    pub name: String,
    pub base: String,
    /// Some(true) hides the server address, Some(false) shows it, None follows the global setting.
    pub hide_address: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Style {
    /// A font stack id from the built-ins, or the name of an imported font.
    pub font: String,
    /// Text size in pixels.
    pub font_size: u32,
    /// "glass", "flat", "outlined" or "floating"
    pub menu: String,
    /// "outline", "bold" or "thin"
    pub icons: String,
    /// "classic", "blue", "mono" or "accent"
    pub folder_color: String,
    /// "plain", "gradient", "aurora", "dusk", "mist" or "image"
    pub background: String,
    /// Imported image used when `background` is "image".
    pub background_image: String,
    /// Corner roundness in pixels.
    pub radius: u32,
    /// Imported style sheets that are switched on.
    pub enabled_sheets: Vec<String>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            font: "system".into(),
            font_size: 13,
            menu: "glass".into(),
            icons: "outline".into(),
            folder_color: "classic".into(),
            background: "gradient".into(),
            background_image: String::new(),
            radius: 8,
            enabled_sheets: Vec::new(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: "dark".into(),
            accent: "#4cc2ff".into(),
            density: "compact".into(),
            custom_css: String::new(),
            drive_chart: "bar".into(),
            drive_metric: "used".into(),
            show_details_pane: true,
            view_level: 0,
            sidebar_width: 220,
            sidebar_collapsed: false,
            show_frequent: true,
            show_pinned: true,
            show_drives: true,
            show_drive_space: true,
            sidebar_drive_space: false,
            show_local_drives: true,
            show_network_drives: true,
            hide_network_address: true,
            drive_names: Default::default(),
            layout: serde_json::Value::Null,
            style: Style::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Performance {
    /// "dynamic": adapt to CPU, RAM and battery. "manual": use the limits below as given.
    pub priority: String,
    /// 0 = one per logical CPU.
    pub max_threads: usize,
    /// Upper bound for index and listing caches. 0 = no limit.
    pub max_cache_mb: usize,
    /// The most of the machine's processing power the app's worker threads
    /// may use, as a percentage of all logical CPUs. 0 = no cap.
    pub max_cpu_percent: u8,
    /// The most of the machine's memory the app's caches may hold, as a
    /// percentage of installed RAM. 0 = no cap.
    pub max_memory_percent: u8,
    pub background_indexing: bool,
    /// Index every mounted local drive instead of only the home folder.
    /// Index every local drive (the default) instead of only the home folder.
    pub index_drives: bool,
    pub lazy_loading: bool,
    pub metadata_caching: bool,
    pub thumbnails: bool,
    pub thumbnail_size: u32,
    pub predictive_prefetch: bool,
    pub reduce_on_battery: bool,
    /// What the app does while it is minimized, hidden or (aggressive only) behind other windows:
    /// "off" keeps working as usual, "gentle" halves the work, "aggressive" drops to the minimum.
    pub background_mode: String,
    /// The person chose `background_mode` themselves, so applying a performance preset leaves it alone.
    pub background_locked: bool,
    /// Native graphics backend for rendering ("auto", "directx12", "directx11", "vulkan", "opengl", "metal", "software").
    pub render_backend: String,
    /// Use the system GPU for compositing, scrolling and painting.
    pub hardware_acceleration: bool,
    /// Rasterize DOM layers, thumbnails and text on the GPU.
    pub gpu_rasterization: bool,
    /// Upload decoded image/thumbnail textures straight into GPU memory buffers.
    pub zero_copy: bool,
    /// Synchronize frames with the display refresh rate; turn off for uncapped frame pacing.
    pub vsync: bool,
    /// OS process scheduling priority ("auto", "low", "normal", "high").
    pub process_priority: String,
    /// Keep rendering and process-priority settings unchanged when switching performance presets.
    pub rendering_locked: bool,
    /// Arbitrary setting keys pinned by the user so performance presets do not overwrite them.
    pub locked_keys: Vec<String>,
}

impl Default for Performance {
    fn default() -> Self {
        Performance {
            priority: "dynamic".into(),
            max_threads: 0,
            max_cache_mb: 256,
            max_cpu_percent: 0,
            max_memory_percent: 0,
            background_indexing: true,
            index_drives: true,
            lazy_loading: true,
            metadata_caching: true,
            thumbnails: true,
            thumbnail_size: 128,
            predictive_prefetch: true,
            reduce_on_battery: true,
            background_mode: "aggressive".into(),
            background_locked: false,
            render_backend: "auto".into(),
            hardware_acceleration: true,
            gpu_rasterization: true,
            zero_copy: true,
            vsync: true,
            process_priority: "auto".into(),
            rendering_locked: false,
            locked_keys: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Transfer {
    /// "ask", "skip", "overwrite" or "keepBoth"
    pub conflict: String,
    pub preserve_timestamps: bool,
    pub large_file_threshold_mb: u64,
    /// 0 = decide automatically per job.
    pub max_parallel: usize,
    /// Learn from finished transfers to predict later ones.
    pub remember_device_speeds: bool,
    /// How many finished transfers are kept for that. 0 keeps none.
    pub history_limit: usize,
    /// "auto", "native", "robocopy" or "rsync". Auto picks from past results.
    pub method: String,
    /// Occasionally try another available method on a big job, to learn which is faster here.
    pub try_other_methods: bool,
    /// Milliseconds between progress updates. 0 = automatic (by device power).
    pub progress_interval_ms: u64,
    /// The most all transfers together may move, in KB per second. 0 = no limit.
    pub max_speed_kb: u64,
    /// The speed each new transfer is held to, in KB per second. 0 = none. Changeable per transfer.
    pub target_speed_kb: u64,
    /// Longest a transfer may work before `on_time_limit` applies, in minutes. 0 = no limit.
    pub max_minutes: u64,
    /// "stop" or "pause" when a transfer reaches its time limit.
    pub on_time_limit: String,
    /// "low", "normal" or "high": the order of the queue, and low yields to other programs.
    pub default_priority: String,
    /// How many transfers work at once; the rest wait their turn. 0 = all of them.
    pub max_active: usize,
    /// Transfers that use the same hard disk take turns; everything else works at the same time.
    pub same_disk_turns: bool,
}

impl Default for Transfer {
    fn default() -> Self {
        Transfer {
            conflict: "ask".into(),
            preserve_timestamps: true,
            large_file_threshold_mb: 32,
            max_parallel: 0,
            remember_device_speeds: true,
            history_limit: 300,
            method: "auto".into(),
            try_other_methods: true,
            progress_interval_ms: 0,
            max_speed_kb: 0,
            target_speed_kb: 0,
            max_minutes: 0,
            on_time_limit: "pause".into(),
            default_priority: "normal".into(),
            max_active: 0,
            same_disk_turns: true,
        }
    }
}

/// How the app keeps itself up to date from the builds its repository publishes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Updates {
    /// Look for new builds at all.
    pub enabled: bool,
    /// Look once each time the app opens.
    pub check_on_open: bool,
    /// Keep looking while the app stays open.
    pub background: bool,
    /// Hours between looks while it stays open.
    pub check_hours: u32,
    /// Download a new build as soon as it is found (otherwise only when asked).
    pub auto_download: bool,
    /// Say so in the notifications when a new build comes out (once for each).
    pub notify: bool,
    /// Also look at the artifacts of the repository's workflow runs (unreleased builds, downloaded through
    /// GitHub with a token or through nightly.link). Off: only the Releases are used.
    pub include_unreleased: bool,
    /// "owner/name" of the repository whose builds are used.
    pub repo: String,
    pub branch: String,
    /// A GitHub access token that may read Actions, needed to download a build straight from GitHub.
    /// Without one, downloads go through nightly.link where the repository allows it.
    pub token: String,
}

impl Default for Updates {
    fn default() -> Self {
        Updates {
            enabled: true,
            check_on_open: true,
            background: true,
            check_hours: 6,
            auto_download: true,
            notify: true,
            include_unreleased: false,
            repo: crate::update::DEFAULT_REPO.into(),
            branch: crate::update::DEFAULT_BRANCH.into(),
            token: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Search {
    pub limit: usize,
    pub include_hidden: bool,
    /// Search the current folder's subtree instead of everything indexed.
    pub scope_to_current_folder: bool,
}

impl Default for Search {
    fn default() -> Self {
        Search { limit: 500, include_hidden: false, scope_to_current_folder: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextMenu {
    /// Ids of entries hidden from the right-click menu.
    pub hidden: Vec<String>,
}

impl Default for ContextMenu {
    fn default() -> Self {
        ContextMenu { hidden: Vec::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Terminal {
    /// "cmd", "powershell" or "wt" (Windows Terminal)
    pub windows_shell: String,
    /// Terminal program on Linux. Empty = detect.
    pub linux_terminal: String,
    /// Terminal app on macOS.
    pub mac_terminal: String,
    /// A command to run as soon as the terminal opens, e.g. `git status`.
    pub startup_command: String,
    /// Replaces the choices above: the whole command to run, `{path}` standing for the folder.
    pub custom_command: String,
    /// Windows Terminal: the profile to open, by name. Empty = its default.
    pub windows_profile: String,
    /// The shell to run inside the terminal on Linux and macOS (bash, zsh, fish…). Empty = the login shell.
    pub shell_program: String,
    /// Open a tab in the terminal window that is already open, where the terminal can.
    pub new_tab: bool,
    /// When one folder is selected, open the terminal in it instead of in the folder being viewed.
    pub use_selected_folder: bool,
    /// Which folder the plain "Open terminal" button opens: "here" or "home".
    pub default_folder: String,
}

impl Default for Terminal {
    fn default() -> Self {
        Terminal {
            windows_shell: "cmd".into(),
            linux_terminal: String::new(),
            mac_terminal: "Terminal".into(),
            startup_command: String::new(),
            custom_command: String::new(),
            windows_profile: String::new(),
            shell_program: String::new(),
            new_tab: false,
            use_selected_folder: true,
            default_folder: "here".into(),
        }
    }
}

impl Settings {
    /// Load settings, falling back to defaults when the file is missing or unreadable.
    pub fn load(file: &Path) -> Settings {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        write_json(file, self)
    }
}

/// Write JSON atomically (temp file + rename) so a crash cannot corrupt it.
///
/// Two saves of the same file can arrive at once (a screen that records several things as it opens). They used to share
/// one temp file, so the second rename found nothing to rename and reported the real file as "not found". Writes are now
/// taken one at a time, each with a temp file of its own.
pub fn write_json<T: Serialize>(file: &Path, value: &T) -> Result<()> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = file.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    let tmp = file.with_file_name(name);
    let data = serde_json::to_vec_pretty(value).map_err(|e| CoreError::Invalid(e.to_string()))?;
    std::fs::write(&tmp, data).map_err(|e| CoreError::io(&tmp, e))?;
    std::fs::rename(&tmp, file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        CoreError::io(file, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_the_same_file_at_once_never_fails() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("cfg/settings.json");
        let handles: Vec<_> = (0..16)
            .map(|i| {
                let f = file.clone();
                std::thread::spawn(move || {
                    for j in 0..20 {
                        let mut s = Settings::default();
                        s.general.show_hidden = (i + j) % 2 == 0;
                        s.save(&f).expect("a save must not fail because another save ran at the same moment");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let _ = Settings::load(&file);
        let leftovers = std::fs::read_dir(file.parent().unwrap()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn partial_files_fill_in_defaults() {
        let s: Settings = serde_json::from_str(r#"{"general":{"showHidden":true},"unknown":1}"#).unwrap();
        assert!(s.general.show_hidden);
        assert!(s.general.delete_to_trash);
        assert_eq!(s.performance.max_cache_mb, 256);
    }

    #[test]
    fn old_settings_files_still_load() {
        // A file from before view_level / history_limit / native_properties existed.
        let s: Settings = serde_json::from_str(
            r#"{"appearance":{"theme":"light","defaultView":"grid"},"transfer":{"conflict":"skip"}}"#,
        )
        .unwrap();
        assert_eq!(s.appearance.theme, "light");
        assert_eq!(s.appearance.view_level, 0);
        assert_eq!(s.transfer.conflict, "skip");
        assert_eq!(s.transfer.history_limit, 300);
        assert!(s.general.native_properties);
    }

    #[test]
    fn roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("cfg/settings.json");
        let mut s = Settings::default();
        s.appearance.custom_css = ".x{color:red}".into();
        s.save(&f).unwrap();
        assert_eq!(Settings::load(&f), s);
        assert_eq!(Settings::load(&d.path().join("missing.json")), Settings::default());
    }
}
