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
    /// Saved sets of settings, switchable in one click.
    pub profiles: Vec<Profile>,
    /// The id of the profile in use, if any.
    pub active_profile: String,
    /// False until the person has been through the first-run setup.
    pub setup_done: bool,
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
pub fn write_json<T: Serialize>(file: &Path, value: &T) -> Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    let tmp = file.with_extension("tmp");
    let data = serde_json::to_vec_pretty(value).map_err(|e| CoreError::Invalid(e.to_string()))?;
    std::fs::write(&tmp, data).map_err(|e| CoreError::io(&tmp, e))?;
    std::fs::rename(&tmp, file).map_err(|e| CoreError::io(file, e))
}

#[cfg(test)]
mod tests {
    use super::*;

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
