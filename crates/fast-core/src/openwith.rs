//! Choosing a program to open a file with.
//!
//! Opening a file normally hands it to the system's default program. When the
//! system has none for that kind of file (or the program is gone), the person
//! is asked instead of the open silently doing nothing. This module answers
//! three questions: *can this file be opened as things stand* (`check`), *which
//! programs could open it* (`choices`), and *open it with this one*
//! (`open_with`, optionally making that the default).
//!
//! Linux reads the desktop entries and `xdg-mime`. Windows reads the registry
//! through `reg`. macOS lists applications and opens with `open -a`; it has no
//! way to ask whether a default exists, so `check` is always `Ready` there (the
//! system shows its own chooser). The Windows and macOS parts are written
//! against those systems' documented behavior and have not been run.

use crate::error::{CoreError, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    /// What to pass back to `open_with`: a desktop entry id, or a program path.
    pub id: String,
    pub name: String,
    /// A small picture as a data: URL, when one could be found.
    pub icon_url: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Choices {
    /// ".zip", or "" for a file without an extension.
    pub extension: String,
    pub default: Option<App>,
    pub suggested: Vec<App>,
    pub others: Vec<App>,
    /// Editors and terminals that can open nearly anything (Visual Studio Code, Notepad, PowerShell...), best first.
    pub general: Vec<App>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// Opening should work.
    Ready,
    /// It will not, and this says why in a sentence.
    Missing(String),
}

/// Programs people usually open each kind of file with, best first. Names are
/// matched against the installed programs' display names and program names
/// (`code` matches "Code" and "Visual Studio Code" but not "Barcode Scanner").
/// Extensions without ".".
const LIKELY: &[(&[&str], &[&str])] = &[
    (
        &["png", "jpg", "jpeg", "jfif", "gif", "bmp", "webp", "tif", "tiff", "ico", "heic", "heif", "avif", "raw", "cr2", "nef", "dng"],
        &["photos", "irfanview", "i_view64", "i_view32", "xnview", "xnviewmp", "paint.net", "paintdotnet", "mspaint", "gimp", "krita", "eog", "eye of gnome", "loupe", "gwenview", "ristretto", "shotwell", "nomacs", "geeqie", "feh", "imv", "preview", "photoshop", "affinity", "chrome", "firefox", "msedge", "edge"],
    ),
    (&["svg"], &["inkscape", "illustrator", "affinity", "gimp", "krita", "chrome", "firefox", "msedge", "edge"]),
    (&["psd", "xcf", "kra"], &["photoshop", "gimp", "krita", "affinity", "paint.net"]),
    (&["ai", "eps"], &["illustrator", "inkscape", "affinity"]),
    (
        &["mp4", "mkv", "avi", "mov", "wmv", "webm", "flv", "m4v", "mpg", "mpeg", "ts", "3gp", "ogv", "vob"],
        &["vlc", "mpv", "mpc-hc", "mpc-be", "potplayer", "movies", "media player", "wmplayer", "totem", "celluloid", "haruna", "smplayer", "parole", "dragon", "quicktime", "iina", "kodi"],
    ),
    (
        &["mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "wma", "opus", "aiff", "aif", "mid", "midi"],
        &["vlc", "foobar2000", "musicbee", "aimp", "winamp", "groove", "rhythmbox", "audacious", "clementine", "strawberry", "elisa", "amarok", "lollypop", "itunes", "music", "mpv", "wmplayer", "media player"],
    ),
    (&["srt", "vtt", "ass", "sub"], &["code", "sublime", "notepad++", "notepad", "gedit", "kate", "text editor", "mousepad", "textedit"]),
    (&["pdf"], &["acrobat", "acrord32", "sumatrapdf", "sumatra", "foxit", "foxitreader", "evince", "okular", "zathura", "atril", "xreader", "mupdf", "masterpdf", "chrome", "firefox", "msedge", "edge", "preview"]),
    (&["epub", "mobi", "azw3", "djvu", "cbz", "cbr"], &["calibre", "ebook", "foliate", "okular", "sumatrapdf", "sumatra", "evince", "koreader"]),
    (&["doc", "docx", "docm", "dot", "dotx", "odt", "rtf"], &["winword", "word", "libreoffice writer", "lowriter", "writer", "wps", "onlyoffice", "abiword", "wordpad", "textedit", "pages"]),
    (&["xls", "xlsx", "xlsm", "xlsb", "ods", "csv", "tsv"], &["excel", "libreoffice calc", "localc", "calc", "wps", "onlyoffice", "gnumeric", "numbers"]),
    (&["ppt", "pptx", "pptm", "pps", "ppsx", "odp"], &["powerpnt", "powerpoint", "libreoffice impress", "loimpress", "impress", "wps", "onlyoffice", "keynote"]),
    (
        &["txt", "md", "markdown", "log", "ini", "cfg", "conf", "config", "json", "jsonc", "xml", "yaml", "yml", "toml", "env", "gitignore", "properties", "sql", "sh", "bash", "bat", "cmd", "ps1", "py", "js", "mjs", "ts", "tsx", "jsx", "css", "scss", "html", "htm", "c", "h", "cpp", "hpp", "cs", "java", "kt", "go", "rs", "rb", "php", "swift", "lua", "pl", "r", "tex", "diff", "patch"],
        &["code", "visual studio code", "vscodium", "sublime", "notepad++", "notepad", "gedit", "text editor", "kate", "kwrite", "mousepad", "xed", "pluma", "featherpad", "textedit", "atom", "geany", "wordpad"],
    ),
    (&["html", "htm", "xhtml", "mhtml", "url"], &["chrome", "firefox", "msedge", "edge", "brave", "chromium", "vivaldi", "opera", "safari", "epiphany", "falkon"]),
    (
        &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "cab", "iso", "img"],
        &["7zfm", "7-zip", "winrar", "peazip", "bandizip", "file-roller", "archive manager", "ark", "xarchiver", "engrampa", "keka", "unarchiver", "archive utility"],
    ),
    (&["ttf", "otf", "woff", "woff2", "ttc"], &["font viewer", "gnome-font-viewer", "font book", "fontforge", "fontview"]),
    (&["torrent"], &["qbittorrent", "transmission", "deluge", "utorrent", "bittorrent", "ktorrent"]),
    (&["stl", "obj", "3mf", "fbx", "blend", "gltf", "glb"], &["blender", "meshlab", "cura", "prusaslicer", "3d viewer", "freecad"]),
    (&["sqlite", "sqlite3", "db"], &["db browser", "sqlitebrowser", "dbeaver", "datagrip", "tableplus"]),
    (&["ics", "ical", "vcf"], &["outlook", "thunderbird", "evolution", "calendar", "contacts", "kontact"]),
    (&["eml", "msg", "mbox"], &["outlook", "thunderbird", "evolution", "mail", "kmail", "claws"]),
    (&["pem", "key", "pub", "ppk", "crt", "cer"], &["code", "notepad++", "notepad", "gedit", "kate", "text editor", "mousepad", "textedit"]),
    (&["exe", "msi"], &[]),
];

/// Does a program called `name` (display name or program name) fit the wanted `keyword`?
fn name_matches(name: &str, keyword: &str) -> bool {
    let name = name.to_lowercase();
    let name = name.trim_end_matches(".exe").trim_end_matches(".desktop");
    if keyword.contains([' ', '-', '.', '+', '_']) {
        return name.contains(keyword);
    }
    // One plain word: it has to be a whole word of the name ("code" in "Visual Studio Code"),
    // or the start of a longer one when it is long enough to be specific ("libreoffice" in "libreoffice-writer").
    name.split(|c: char| !c.is_alphanumeric() && c != '+').any(|w| w == keyword || (keyword.len() >= 5 && w.starts_with(keyword)))
}

/// The installed program that most likely suits this kind of file: the best
/// match from the list of usual programs among `programs` (suggested ones
/// first, since the system itself says those handle the file type), else the
/// first program the system suggests for it. `None` leaves the choice to the person.
pub fn likely_app(choices: &Choices) -> Option<App> {
    let ext = choices.extension.trim_start_matches('.').to_lowercase();
    if ext.is_empty() {
        return None;
    }
    let wanted: Vec<&str> = LIKELY.iter().filter(|(exts, _)| exts.contains(&ext.as_str())).flat_map(|(_, names)| names.iter().copied()).collect();
    let pools: [&[App]; 2] = [&choices.suggested, &choices.others];
    for pool in pools {
        for keyword in &wanted {
            if let Some(app) = pool.iter().find(|a| name_matches(&a.name, keyword) || name_matches(&a.id.rsplit(['/', '\\']).next().unwrap_or(&a.id), keyword)) {
                return Some(app.clone());
            }
        }
        // Programs the system lists as made for this type are a good answer even if not in the table.
        if std::ptr::eq(pool, &choices.suggested[..]) {
            if let Some(first) = pool.first() {
                return Some(first.clone());
            }
        }
    }
    None
}

/// True for a command like `"C:\Gone\player.exe" "%1"` whose program is not on disk.
/// Anything that is not a plain path to a program (a Store app, a `(value not set)`
/// placeholder, a command found on PATH) is left to Windows.
#[allow(dead_code)] // the Windows check uses it
pub fn names_missing_program(cmd: &str) -> bool {
    let first = split_command(cmd).into_iter().next().unwrap_or_default();
    let lower = first.to_lowercase();
    let absolute = first.len() > 3 && first.as_bytes()[1] == b':' && (first.as_bytes()[2] == b'\\' || first.as_bytes()[2] == b'/');
    absolute && lower.ends_with(".exe") && !Path::new(&first).is_file()
}


pub fn extension_of(path: &Path) -> String {
    path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

#[allow(dead_code)] // used on some platforms only
fn what(path: &Path) -> String {
    let ext = extension_of(path);
    if ext.is_empty() { "this kind of file".into() } else { format!("{ext} files") }
}

/// Is `program` (a name looked up on PATH, or a path) runnable?
pub fn program_exists(program: &str) -> bool {
    let p = Path::new(program);
    if p.is_absolute() || program.contains('/') || program.contains('\\') {
        return p.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|d| {
                let f = d.join(program);
                f.is_file() || (cfg!(windows) && d.join(format!("{program}.exe")).is_file())
            })
        })
        .unwrap_or(false)
}

/// Split a command line the way a shell would for plain arguments: spaces
/// separate, quotes group, a backslash escapes the next character.
pub fn split_command(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => match chars.peek().copied() {
                // Inside quotes only these are escapes; anything else keeps its backslash (Windows paths).
                Some(n @ ('"' | '`' | '$' | '\\')) => {
                    chars.next();
                    cur.push(n);
                }
                _ => cur.push('\\'),
            },
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') if cfg!(unix) => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[allow(dead_code)] // used on some platforms only
fn encode_icon(file: &Path) -> Option<String> {
    use base64::Engine;
    let ext = file.extension()?.to_string_lossy().to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => return None,
    };
    let meta = std::fs::metadata(file).ok()?;
    if meta.len() > 200_000 {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

// ===========================================================================
// Linux and other Unix without a desktop database of its own
// ===========================================================================

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
mod platform {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    #[derive(Clone, Debug)]
    pub(super) struct Entry {
        pub id: String,
        pub name: String,
        pub exec: String,
        pub icon: String,
        pub mimes: Vec<String>,
        pub terminal: bool,
    }

    pub(super) fn app_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(d) = dirs::data_dir() {
            dirs.push(d.join("applications"));
        }
        let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        for d in system.split(':').filter(|d| !d.is_empty()) {
            dirs.push(Path::new(d).join("applications"));
        }
        if let Some(h) = dirs::home_dir() {
            dirs.push(h.join(".local/share/flatpak/exports/share/applications"));
        }
        dirs.push("/var/lib/flatpak/exports/share/applications".into());
        dirs.push("/var/lib/snapd/desktop/applications".into());
        dirs
    }

    /// The `[Desktop Entry]` group of a .desktop file.
    pub(super) fn parse_entry(id: &str, text: &str) -> Option<Entry> {
        let mut in_main = false;
        let (mut name, mut exec, mut icon, mut mimes) = (String::new(), String::new(), String::new(), Vec::new());
        let (mut hidden, mut terminal, mut is_app) = (false, false, false);
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_main = line == "[Desktop Entry]";
                continue;
            }
            if !in_main || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "Name" => name = v.trim().to_string(),
                "Exec" => exec = v.trim().to_string(),
                "Icon" => icon = v.trim().to_string(),
                "MimeType" => mimes = v.split(';').filter(|m| !m.is_empty()).map(str::to_string).collect(),
                "NoDisplay" | "Hidden" => hidden |= v.trim() == "true",
                "Terminal" => terminal = v.trim() == "true",
                "Type" => is_app = v.trim() == "Application",
                _ => {}
            }
        }
        (is_app && !hidden && !name.is_empty() && !exec.is_empty()).then(|| Entry { id: id.to_string(), name, exec, icon, mimes, terminal })
    }

    pub(super) fn installed() -> Vec<Entry> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for dir in app_dirs() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.filter_map(|e| e.ok()) {
                let file = e.file_name().to_string_lossy().into_owned();
                if !file.ends_with(".desktop") || !seen.insert(file.clone()) {
                    continue;
                }
                if let Some(entry) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_entry(&file, &t)) {
                    out.push(entry);
                }
            }
        }
        out
    }

    pub(super) fn find(id: &str) -> Option<Entry> {
        installed().into_iter().find(|e| e.id == id)
    }

    /// The program an entry would start, for "is it installed".
    pub(super) fn program_of(exec: &str) -> Option<String> {
        let words = split_command(exec);
        let mut it = words.into_iter().skip_while(|w| w == "env" || w.contains('=') && !w.starts_with('/'));
        it.next()
    }

    pub(super) fn mime_of(path: &Path) -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "filetype"]).arg(path).output().ok()?;
        let m = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !m.is_empty()).then_some(m)
    }

    fn default_id(mime: &str) -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "default", mime]).output().ok()?;
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!id.is_empty()).then_some(id)
    }

    /// Programs and scripts are run by the system, not opened with a viewer.
    fn is_program_type(mime: &str) -> bool {
        matches!(mime, "application/x-executable" | "application/x-pie-executable" | "application/x-sharedlib" | "inode/directory")
    }

    pub(super) fn icon_for(name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        if name.starts_with('/') {
            return encode_icon(Path::new(name));
        }
        let mut bases: Vec<PathBuf> = vec!["/usr/share/icons".into(), "/usr/share/pixmaps".into()];
        if let Some(d) = dirs::data_dir() {
            bases.push(d.join("icons"));
        }
        for base in &bases {
            for size in ["48x48", "64x64", "32x32", "128x128", "256x256", "scalable"] {
                for ext in ["png", "svg"] {
                    let f = base.join("hicolor").join(size).join("apps").join(format!("{name}.{ext}"));
                    if f.is_file() {
                        return encode_icon(&f);
                    }
                }
            }
            for ext in ["png", "svg"] {
                let f = base.join(format!("{name}.{ext}"));
                if f.is_file() {
                    return encode_icon(&f);
                }
            }
        }
        None
    }

    fn app_of(e: &Entry, with_icon: bool) -> App {
        App { id: e.id.clone(), name: e.name.clone(), icon_url: if with_icon { icon_for(&e.icon) } else { None } }
    }

    pub fn check(path: &Path) -> Check {
        if path.is_dir() {
            return Check::Ready;
        }
        let Some(mime) = mime_of(path) else { return Check::Ready }; // no xdg-mime: leave it to the opener
        if is_program_type(&mime) {
            return Check::Ready;
        }
        let Some(id) = default_id(&mime) else {
            return Check::Missing(format!("No app is set to open {}.", what(path)));
        };
        match find(&id) {
            None => Check::Missing(format!("The app set to open {} ({id}) is not installed.", what(path))),
            Some(e) => match program_of(&e.exec) {
                Some(p) if program_exists(&p) => Check::Ready,
                _ => Check::Missing(format!("{} cannot be found on this computer.", e.name)),
            },
        }
    }

    pub fn choices(path: &Path) -> Choices {
        let mime = mime_of(path).unwrap_or_default();
        let default_entry = default_id(&mime).and_then(|id| find(&id)).filter(|e| program_of(&e.exec).is_some_and(|p| program_exists(&p)));
        let parent = if mime.starts_with("text/") { "text/plain" } else { "" };
        let mut suggested = Vec::new();
        let mut others = Vec::new();
        let mut names = HashSet::new();
        let mut all = installed();
        all.sort_by_key(|e| e.name.to_lowercase());
        for e in all {
            if default_entry.as_ref().is_some_and(|d| d.id == e.id) || !names.insert(e.name.clone()) {
                continue;
            }
            if !program_of(&e.exec).is_some_and(|p| program_exists(&p)) {
                continue;
            }
            let fits = !mime.is_empty() && e.mimes.iter().any(|m| *m == mime || (!parent.is_empty() && m == parent));
            let app = app_of(&e, fits || suggested.len() + others.len() < 60);
            if fits { suggested.push(app) } else { others.push(app) }
        }
        Choices { extension: extension_of(path), default: default_entry.map(|e| app_of(&e, true)), suggested, others, general: vec![] }
    }

    /// Expand a desktop entry's Exec line for one file.
    pub(super) fn command_for(e: &Entry, path: &Path) -> Vec<String> {
        let file = path.to_string_lossy().into_owned();
        let mut argv = Vec::new();
        let mut had_file = false;
        for w in split_command(&e.exec) {
            let mut word = String::new();
            let mut chars = w.chars();
            while let Some(c) = chars.next() {
                if c != '%' {
                    word.push(c);
                    continue;
                }
                match chars.next() {
                    Some('f') | Some('F') | Some('u') | Some('U') => {
                        word.push_str(&file);
                        had_file = true;
                    }
                    Some('c') => word.push_str(&e.name),
                    Some('%') => word.push('%'),
                    _ => {} // %i %k %d %D %n %N %m %v: nothing for a plain file
                }
            }
            if !word.is_empty() || w.is_empty() {
                argv.push(word);
            }
        }
        if !had_file {
            argv.push(file);
        }
        argv
    }

    pub fn open_with(path: &Path, id: &str, always: bool) -> Result<()> {
        // A program given by path (from "Browse…") is run directly.
        let entry = if id.starts_with('/') {
            Entry { id: id.into(), name: id.rsplit('/').next().unwrap_or(id).into(), exec: format!("\"{id}\" %f"), icon: String::new(), mimes: vec![], terminal: false }
        } else {
            find(id).ok_or(CoreError::Unsupported("that app is not installed"))?
        };
        let mut argv = command_for(&entry, path);
        if entry.terminal {
            let mut full = vec!["x-terminal-emulator".to_string(), "-e".to_string()];
            full.append(&mut argv);
            argv = full;
        }
        let (program, args) = argv.split_first().ok_or(CoreError::Unsupported("empty command"))?;
        let mut command = Command::new(program);
        if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
            command.current_dir(dir);
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CoreError::io(Path::new(program), e))?;
        // Reap it when it ends so it does not linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        if always && !id.starts_with('/') {
            if let Some(mime) = mime_of(path) {
                let _ = Command::new("xdg-mime").args(["default", id, &mime]).status();
            }
        }
        Ok(())
    }
}

// ===========================================================================
// Windows
// ===========================================================================

#[cfg(windows)]
mod platform {
    use super::*;
    use std::process::Command;

    fn reg(args: &[&str]) -> Option<String> {
        let out = crate::proc::quiet(Command::new("reg").args(args)).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The text after the value type in a `reg query` line.
    fn value_of(output: &str, name: &str) -> Option<String> {
        output.lines().find_map(|l| {
            let t = l.trim();
            let rest = if name.is_empty() { t.strip_prefix("(Default)") } else { t.strip_prefix(name) }?;
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("REG_SZ").or_else(|| rest.strip_prefix("REG_EXPAND_SZ"))?;
            Some(rest.trim().to_string())
        })
    }

    /// A text value from the registry, read directly (no `reg.exe` process: that took a noticeable fraction of a second each time).
    fn reg_str(root: windows_sys::Win32::System::Registry::HKEY, sub: &str, name: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
        let wsub: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
        let wname: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut bytes: u32 = 0;
        // SAFETY: both strings are NUL-terminated and outlive the calls; the buffer is as long as the size asked for.
        unsafe {
            if RegGetValueW(root, wsub.as_ptr(), wname.as_ptr(), flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut bytes) != 0 || bytes < 2 {
                return None;
            }
            let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
            let mut cap = (buf.len() * 2) as u32;
            if RegGetValueW(root, wsub.as_ptr(), wname.as_ptr(), flags, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut cap) != 0 {
                return None;
            }
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let text = String::from_utf16_lossy(&buf[..n]);
            (!text.trim().is_empty()).then_some(text)
        }
    }

    fn prog_id(ext: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
        let user = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}\UserChoice");
        reg_str(HKEY_CURRENT_USER, &user, "ProgId").or_else(|| reg_str(HKEY_CLASSES_ROOT, ext, ""))
    }

    fn open_command(prog: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::HKEY_CLASSES_ROOT;
        reg_str(HKEY_CLASSES_ROOT, &format!(r"{prog}\shell\open\command"), "")
    }

    fn expand_env(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            match rest[i + 1..].find('%') {
                Some(j) => {
                    let var = &rest[i + 1..i + 1 + j];
                    out.push_str(&std::env::var(var).unwrap_or_else(|_| format!("%{var}%")));
                    rest = &rest[i + j + 2..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    pub fn check(path: &Path) -> Check {
        if path.is_dir() {
            return Check::Ready;
        }
        let ext = extension_of(path);
        if ext.is_empty() {
            return Check::Missing("No app is set to open this kind of file.".into());
        }
        if matches!(ext.as_str(), ".exe" | ".bat" | ".cmd" | ".com" | ".msi" | ".lnk" | ".ps1" | ".scr") {
            return Check::Ready;
        }
        let Some(prog) = prog_id(&ext) else { return Check::Missing(format!("No app is set to open {}.", what(path))) };
        // Windows decides. Apps from the Store (Photos, Media Player...) have no plain command to
        // read here, so only a command that names a program file that is really gone counts as missing.
        match open_command(&prog) {
            Some(cmd) if names_missing_program(&expand_env(&cmd)) => Check::Missing(format!("The app set to open {} cannot be found.", what(path))),
            _ => Check::Ready,
        }
    }

    fn app_from_exe(exe: &str, with_name: Option<&str>) -> App {
        let stem = Path::new(exe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| exe.to_string());
        App { id: exe.to_string(), name: with_name.map(str::to_string).unwrap_or(stem), icon_url: None }
    }

    pub fn choices(path: &Path) -> Choices {
        let ext = extension_of(path);
        let mut suggested = Vec::new();
        let mut seen = std::collections::HashSet::new();
        // Programs Windows remembers for this extension.
        if let Some(o) = reg(&["query", &format!(r"HKCR\{ext}\OpenWithList"), "/s"]) {
            for exe in o.lines().filter_map(|l| l.trim().split_whitespace().last()).filter(|e| e.to_lowercase().ends_with(".exe")) {
                if let Some(full) = app_path(exe) {
                    if seen.insert(full.to_lowercase()) {
                        suggested.push(app_from_exe(&full, None));
                    }
                }
            }
        }
        // Programs that registered themselves as able to open this type (how VLC, browsers,
        // viewers and editors say so), and the ones listed for it under OpenWithProgids.
        for app in registered_apps(&ext) {
            if seen.insert(app.id.to_lowercase()) {
                suggested.push(app);
            }
        }
        // Everything registered under App Paths.
        let mut others = Vec::new();
        if let Some(o) = reg(&["query", r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths", "/s", "/ve"]) {
            for exe in o.lines().filter_map(|l| value_of(l, "(Default)")).map(|p| expand_env(p.trim_matches('"'))) {
                if exe.to_lowercase().ends_with(".exe") && Path::new(&exe).is_file() && seen.insert(exe.to_lowercase()) {
                    others.push(app_from_exe(&exe, None));
                }
            }
        }
        others.sort_by_key(|a| a.name.to_lowercase());
        let default = prog_id(&ext).and_then(|p| open_command(&p)).and_then(|c| split_command(&expand_env(&c)).into_iter().next()).filter(|e| Path::new(e).is_file()).map(|e| app_from_exe(&e, None));
        Choices { extension: ext, default, suggested, others, general: vec![] }
    }

    /// Programs that say they can open `ext`: each entry under RegisteredApplications points
    /// at a Capabilities key whose FileAssociations name the ProgId used for that extension.
    fn registered_apps(ext: &str) -> Vec<App> {
        let mut out = Vec::new();
        for hive in ["HKLM", "HKCU"] {
            let Some(list) = reg(&["query", &format!(r"{hive}\SOFTWARE\RegisteredApplications")]) else { continue };
            for line in list.lines() {
                let Some((name, cap)) = line.trim().split_once("REG_SZ") else { continue };
                let (name, cap) = (name.trim(), cap.trim());
                if name.is_empty() || cap.is_empty() {
                    continue;
                }
                let Some(assoc) = reg(&["query", &format!(r"{hive}\{cap}\FileAssociations"), "/v", ext]) else { continue };
                let Some(prog) = value_of(&assoc, ext) else { continue };
                let Some(exe) = open_command(&prog).and_then(|c| split_command(&expand_env(&c)).into_iter().next()) else { continue };
                if !exe.to_lowercase().ends_with(".exe") || !Path::new(&exe).is_file() {
                    continue;
                }
                let shown = reg(&["query", &format!(r"{hive}\{cap}"), "/v", "ApplicationName"])
                    .and_then(|o| value_of(&o, "ApplicationName"))
                    .filter(|n| !n.starts_with('@') && !n.is_empty())
                    .unwrap_or_else(|| name.to_string());
                out.push(App { id: exe, name: shown, icon_url: None });
            }
        }
        out
    }

    fn app_path(exe: &str) -> Option<String> {
        reg(&["query", &format!(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}"), "/ve"])
            .and_then(|o| value_of(&o, ""))
            .map(|p| expand_env(p.trim_matches('"')))
            .filter(|p| Path::new(p).is_file())
    }

    /// A command prompt, PowerShell or Windows Terminal chosen for a file opens a new console in its folder and runs
    /// the file there (a folder is just opened in it).
    fn in_shell(path: &Path, id: &str) -> Option<Result<()>> {
        use std::os::windows::process::CommandExt;
        const NEW_CONSOLE: u32 = 0x0000_0010;
        let stem = Path::new(id).file_stem()?.to_string_lossy().to_lowercase();
        let dir = if path.is_dir() { path } else { path.parent()? };
        let full = path.to_string_lossy().into_owned();
        let quoted = full.replace('\'', "''");
        let ps_command = format!("& '{quoted}'");
        let mut cmd = Command::new(id);
        cmd.current_dir(dir);
        match stem.as_str() {
            "cmd" => {
                cmd.creation_flags(NEW_CONSOLE);
                if !path.is_dir() {
                    cmd.raw_arg(format!("/K \"\"{full}\"\""));
                }
            }
            "powershell" | "pwsh" => {
                cmd.creation_flags(NEW_CONSOLE).arg("-NoExit");
                if !path.is_dir() {
                    cmd.arg("-Command").arg(&ps_command);
                }
            }
            "wt" => {
                cmd.arg("-d").arg(dir);
                if !path.is_dir() {
                    cmd.args(["powershell", "-NoExit", "-Command"]).arg(&ps_command);
                }
            }
            _ => return None,
        }
        Some(cmd.spawn().map(|_| ()).map_err(|e| CoreError::io(Path::new(id), e)))
    }

    pub fn open_with(path: &Path, id: &str, _always: bool) -> Result<()> {
        if let Some(done) = in_shell(path, id) {
            return done;
        }
        let mut cmd = Command::new(id);
        if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
            cmd.current_dir(dir);
        }
        cmd.arg(path).spawn().map(|_| ()).map_err(|e| CoreError::io(Path::new(id), e))
    }

    /// Windows' own "How do you want to open this file?" window.
    pub fn system_chooser(path: &Path) -> Result<()> {
        Command::new("rundll32.exe").arg("shell32.dll,OpenAs_RunDLL").arg(path).spawn().map(|_| ()).map_err(|e| CoreError::io(path, e))
    }
}

// ===========================================================================
// macOS
// ===========================================================================

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::Command;

    pub fn check(_path: &Path) -> Check {
        Check::Ready
    }

    pub fn choices(path: &Path) -> Choices {
        let mut apps = Vec::new();
        let mut roots = vec![std::path::PathBuf::from("/Applications"), "/System/Applications".into()];
        if let Some(h) = dirs::home_dir() {
            roots.push(h.join("Applications"));
        }
        for root in roots {
            if let Ok(rd) = std::fs::read_dir(root) {
                for e in rd.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "app") {
                        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        apps.push(App { id: p.to_string_lossy().into_owned(), name, icon_url: None });
                    }
                }
            }
        }
        apps.sort_by_key(|a| a.name.to_lowercase());
        Choices { extension: extension_of(path), default: None, suggested: vec![], others: apps, general: vec![] }
    }

    pub fn open_with(path: &Path, id: &str, _always: bool) -> Result<()> {
        let mut cmd = Command::new("open");
        if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
            cmd.current_dir(dir);
        }
        cmd.args(["-a", id]).arg(path).spawn().map(|_| ()).map_err(|e| CoreError::io(path, e))
    }
}

// ===========================================================================
// Phones and anything else: the shell does it
// ===========================================================================

#[cfg(any(target_os = "android", target_os = "ios", not(any(unix, windows))))]
mod platform {
    use super::*;
    pub fn check(_: &Path) -> Check {
        Check::Ready
    }
    pub fn choices(path: &Path) -> Choices {
        Choices { extension: extension_of(path), ..Default::default() }
    }
    pub fn open_with(_: &Path, _: &str, _: bool) -> Result<()> {
        Err(CoreError::Unsupported("choosing an app is handled by the mobile shell"))
    }
}

/// Open `path` with the program `id` (see `Choices`). With `always`, that program also becomes the one used from
/// now on where the system lets that be set.
pub fn open_with(path: &Path, id: &str, always: bool) -> Result<()> {
    let done = platform::open_with(path, id, always);
    if always && done.is_ok() {
        forget_openers();
    }
    done
}

pub use platform::check;

// ===========================================================================
// Which program a file opens with (shown next to it, without asking the system for a whole chooser)
// ===========================================================================

/// What happens when a file is opened, for showing next to it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Opener {
    /// "app": a program is set to open it. "runs": it is a program or script the system runs itself.
    /// "missing": a program is set but it is gone. "none": nothing is set.
    pub kind: String,
    pub name: String,
    /// The program: a desktop entry id, or a program path. Empty when there is none.
    pub id: String,
    pub icon_url: Option<String>,
}

impl Opener {
    fn of(kind: &str, name: &str, id: &str) -> Self {
        Opener { kind: kind.into(), name: name.into(), id: id.into(), icon_url: None }
    }

    /// The words shown in a column.
    pub fn text(&self) -> String {
        match self.kind.as_str() {
            "app" => self.name.clone(),
            "runs" => "Runs itself".into(),
            "missing" if self.name.is_empty() => "Program missing".into(),
            "missing" => format!("{} (missing)", self.name),
            _ => "No program set".into(),
        }
    }
}

/// What the system found for a kind of file, before it is dressed up. (Phones find nothing, so there the other
/// variants are never built.)
#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub(crate) enum Found {
    App { id: String, name: String, icon: String },
    Missing { name: String },
    Runs,
    Nothing,
}

const OPENER_TTL: std::time::Duration = std::time::Duration::from_secs(120);
const OPENER_KEEP: usize = 2048;

type OpenerMemory = std::collections::HashMap<String, (std::time::Instant, Opener, String)>;

fn memory() -> &'static std::sync::Mutex<OpenerMemory> {
    static M: std::sync::OnceLock<std::sync::Mutex<OpenerMemory>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Throw away what is remembered about which program opens what (after the default was changed).
pub fn forget_openers() {
    if let Ok(mut m) = memory().lock() {
        m.clear();
    }
}

/// What a file opens with, remembered per extension (a file without one is looked at by name), for two minutes.
/// `with_icon` adds the program's small picture where the system gives one.
pub fn default_opener(path: &Path, with_icon: bool) -> Opener {
    let ext = extension_of(path);
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    #[cfg(unix)]
    if ext.is_empty() {
        use std::os::unix::fs::PermissionsExt;
        // A file without an extension that is marked as runnable is run, whatever it is.
        if std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0) {
            return Opener::of("runs", "", "");
        }
    }
    let key = if ext.is_empty() { format!("n:{name}") } else { ext };
    let hit = memory().lock().ok().and_then(|m| m.get(&key).filter(|(at, _, _)| at.elapsed() < OPENER_TTL).map(|(_, o, icon)| (o.clone(), icon.clone())));
    let (mut opener, icon) = match hit {
        Some(h) => h,
        None => {
            let (o, icon) = match defaults::found(path) {
                Found::App { id, name, icon } => (Opener::of("app", &name, &id), icon),
                Found::Missing { name } => (Opener::of("missing", &name, ""), String::new()),
                Found::Runs => (Opener::of("runs", "", ""), String::new()),
                Found::Nothing => (Opener::of("none", "", ""), String::new()),
            };
            if let Ok(mut m) = memory().lock() {
                if m.len() >= OPENER_KEEP {
                    m.clear();
                }
                m.insert(key, (std::time::Instant::now(), o.clone(), icon.clone()));
            }
            (o, icon)
        }
    };
    if let Some(n) = friendly(&opener.name, &opener.id) {
        opener.name = n.to_string();
    }
    if with_icon && opener.kind == "app" {
        opener.icon_url = defaults::icon(&opener.id, &icon);
    }
    opener
}

mod defaults {
    use super::*;

    pub(super) fn icon(id: &str, icon: &str) -> Option<String> {
        #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
        {
            let _ = id;
            return platform::icon_for(icon);
        }
        #[allow(unreachable_code)]
        {
            let _ = (id, icon);
            None
        }
    }

    // ---- Linux and other Unix with desktop entries ----

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
    pub(super) mod unix {
        use super::super::*;
        use std::collections::HashMap;

        /// One line of the shared MIME database's `globs2`: weight, type, pattern.
        #[derive(Debug, PartialEq)]
        pub struct Glob {
            pub weight: i32,
            pub mime: String,
            pub suffix: Option<String>,
            pub literal: Option<String>,
        }

        pub fn parse_globs(text: &str) -> Vec<Glob> {
            let mut out = Vec::new();
            for line in text.lines() {
                if line.starts_with('#') {
                    continue;
                }
                let mut parts = line.splitn(4, ':');
                let (Some(w), Some(mime), Some(pat)) = (parts.next(), parts.next(), parts.next()) else { continue };
                let Ok(weight) = w.trim().parse::<i32>() else { continue };
                let pat = pat.to_lowercase();
                if let Some(rest) = pat.strip_prefix('*') {
                    if !rest.is_empty() && !rest.contains(['*', '?', '[']) {
                        out.push(Glob { weight, mime: mime.into(), suffix: Some(rest.into()), literal: None });
                    }
                } else if !pat.contains(['*', '?', '[']) {
                    out.push(Glob { weight, mime: mime.into(), suffix: None, literal: Some(pat) });
                }
            }
            out
        }

        /// The type a file name has: the longest matching pattern wins, then the heaviest.
        pub fn mime_for_name(globs: &[Glob], file_name: &str) -> Option<String> {
            let name = file_name.to_lowercase();
            let mut best: Option<(usize, i32, &str)> = None;
            for g in globs {
                let len = match (&g.literal, &g.suffix) {
                    (Some(l), _) if *l == name => name.len() + 1,
                    (_, Some(sfx)) if name.ends_with(sfx.as_str()) && name.len() > sfx.len() => sfx.len(),
                    _ => continue,
                };
                if best.is_none_or(|(bl, bw, _)| (len, g.weight) > (bl, bw)) {
                    best = Some((len, g.weight, &g.mime));
                }
            }
            best.map(|(_, _, m)| m.to_string())
        }

        fn data_dirs() -> Vec<std::path::PathBuf> {
            let mut dirs = Vec::new();
            if let Some(d) = dirs::data_dir() {
                dirs.push(d);
            }
            let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
            dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from));
            dirs
        }

        fn globs() -> &'static Vec<Glob> {
            static G: std::sync::OnceLock<Vec<Glob>> = std::sync::OnceLock::new();
            G.get_or_init(|| data_dirs().iter().filter_map(|d| std::fs::read_to_string(d.join("mime/globs2")).ok()).flat_map(|t| parse_globs(&t)).collect())
        }

        /// `mimeapps.list` and `mimeinfo.cache` contents: type -> desktop entry ids, in order.
        #[derive(Default, Debug, PartialEq)]
        pub struct Associations {
            pub default: HashMap<String, Vec<String>>,
            pub added: HashMap<String, Vec<String>>,
            pub removed: HashMap<String, Vec<String>>,
        }

        pub fn parse_associations(text: &str) -> Associations {
            let mut a = Associations::default();
            let mut section = "";
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    section = match line {
                        "[Default Applications]" => "d",
                        "[Added Associations]" | "[MIME Cache]" => "a",
                        "[Removed Associations]" => "r",
                        _ => "",
                    };
                    continue;
                }
                let Some((mime, ids)) = line.split_once('=') else { continue };
                let ids: Vec<String> = ids.split(';').map(str::trim).filter(|i| !i.is_empty()).map(str::to_string).collect();
                let map = match section {
                    "d" => &mut a.default,
                    "a" => &mut a.added,
                    "r" => &mut a.removed,
                    _ => continue,
                };
                map.entry(mime.trim().to_string()).or_default().extend(ids);
            }
            a
        }

        /// The user's and the system's association files, most specific first, and the systems' lists of what can open each type.
        fn association_files() -> (Vec<Associations>, Vec<Associations>) {
            let mut paths = Vec::new();
            let config_home = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| dirs::home_dir().map(|h| h.join(".config")));
            let mut config_dirs: Vec<std::path::PathBuf> = config_home.into_iter().collect();
            let system = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".into());
            config_dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from));
            let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').filter(|d| !d.is_empty()).map(str::to_lowercase).collect();
            for dir in &config_dirs {
                paths.extend(desktops.iter().map(|d| dir.join(format!("{d}-mimeapps.list"))));
                paths.push(dir.join("mimeapps.list"));
            }
            for dir in data_dirs() {
                paths.extend(desktops.iter().map(|d| dir.join(format!("applications/{d}-mimeapps.list"))));
                paths.push(dir.join("applications/mimeapps.list"));
            }
            let lists = paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()).map(|t| parse_associations(&t)).collect();
            let caches = data_dirs().iter().filter_map(|d| std::fs::read_to_string(d.join("applications/mimeinfo.cache")).ok()).map(|t| parse_associations(&t)).collect();
            (lists, caches)
        }

        /// The desktop entry that opens `mime`, by the freedesktop rules: a chosen default, then anything added for it,
        /// then whatever the systems' caches list for the type; entries that are not installed, or were removed for the
        /// type, are skipped.
        pub fn pick(mime: &str, files: &[Associations], caches: &[Associations], installed: &dyn Fn(&str) -> bool) -> Option<String> {
            let removed = |id: &str| files.iter().any(|f| f.removed.get(mime).is_some_and(|r| r.iter().any(|x| x == id)));
            let usable = |id: &&String| !removed(id) && installed(id);
            files
                .iter()
                .filter_map(|f| f.default.get(mime))
                .flatten()
                .find(usable)
                .or_else(|| files.iter().filter_map(|f| f.added.get(mime)).flatten().find(usable))
                .or_else(|| caches.iter().filter_map(|f| f.added.get(mime)).flatten().find(usable))
                .cloned()
        }

        fn entry_for(id: &str) -> Option<platform::Entry> {
            for dir in platform::app_dirs() {
                if let Ok(text) = std::fs::read_to_string(dir.join(id)) {
                    if let Some(e) = platform::parse_entry(id, &text) {
                        return Some(e);
                    }
                }
            }
            None
        }

        fn mime_of(path: &Path) -> Option<String> {
            let name = path.file_name()?.to_string_lossy();
            mime_for_name(globs(), &name).or_else(|| platform::mime_of(path))
        }

        pub fn found(path: &Path) -> Found {
            let Some(mime) = mime_of(path) else { return Found::Nothing };
            if matches!(mime.as_str(), "application/x-executable" | "application/x-pie-executable") {
                return Found::Runs;
            }
            let (files, caches) = association_files();
            let installed = |id: &str| entry_for(id).is_some();
            let id = pick(&mime, &files, &caches, &installed).or_else(|| {
                // Text of every kind falls back to what opens plain text.
                (mime.starts_with("text/") && mime != "text/plain").then(|| pick("text/plain", &files, &caches, &installed)).flatten()
            });
            let Some(id) = id else { return Found::Nothing };
            match entry_for(&id) {
                None => Found::Missing { name: String::new() },
                Some(e) if platform::program_of(&e.exec).is_some_and(|p| program_exists(&p)) => Found::App { id: e.id, name: e.name, icon: e.icon },
                Some(e) => Found::Missing { name: e.name },
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
    pub(super) use unix::found;

    // ---- Windows: ask the shell, which knows about Store apps and per-user choices ----

    #[cfg(windows)]
    pub(super) fn found(path: &Path) -> Found {
        use windows_sys::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE, ASSOCSTR_FRIENDLYAPPNAME};
        let ext = extension_of(path);
        if ext.is_empty() {
            return Found::Nothing;
        }
        if matches!(ext.as_str(), ".exe" | ".com" | ".bat" | ".cmd" | ".scr" | ".lnk") {
            return Found::Runs;
        }
        let ask = |what: i32| -> Option<String> {
            let wext: Vec<u16> = ext.encode_utf16().chain(std::iter::once(0)).collect();
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            // SAFETY: the extension is NUL-terminated, the buffer is as long as `len` says, and nothing outlives the call.
            let hr = unsafe { AssocQueryStringW(ASSOCF_NONE, what, wext.as_ptr(), std::ptr::null(), buf.as_mut_ptr(), &mut len) };
            if hr != 0 {
                return None;
            }
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let text = String::from_utf16_lossy(&buf[..n]);
            (!text.trim().is_empty()).then_some(text)
        };
        match ask(ASSOCSTR_FRIENDLYAPPNAME) {
            Some(name) => Found::App { id: ask(ASSOCSTR_EXECUTABLE).unwrap_or_default(), name, icon: String::new() },
            None => Found::Nothing,
        }
    }

    // ---- macOS: Launch Services ----

    #[cfg(target_os = "macos")]
    pub(super) fn found(path: &Path) -> Found {
        use core_foundation::base::TCFType;
        use core_foundation::url::{CFURL, CFURLRef};
        #[link(name = "CoreServices", kind = "framework")]
        extern "C" {
            fn LSCopyDefaultApplicationURLForURL(url: CFURLRef, role_mask: u32, error: *mut std::ffi::c_void) -> CFURLRef;
        }
        const ALL_ROLES: u32 = 0xFFFF_FFFF;
        let Some(url) = CFURL::from_path(path, false) else { return Found::Nothing };
        // SAFETY: `url` is a live CFURL; the call returns a new reference (or null), which is wrapped to be released.
        let app = unsafe { LSCopyDefaultApplicationURLForURL(url.as_concrete_TypeRef(), ALL_ROLES, std::ptr::null_mut()) };
        if app.is_null() {
            return Found::Nothing;
        }
        // SAFETY: non-null, returned under the Create rule.
        let app = unsafe { CFURL::wrap_under_create_rule(app) };
        match app.to_path() {
            Some(p) => Found::App { name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), id: p.to_string_lossy().into_owned(), icon: String::new() },
            None => Found::Nothing,
        }
    }

    // ---- Phones and anything else ----

    #[cfg(any(target_os = "android", target_os = "ios", not(any(unix, windows))))]
    pub(super) fn found(_: &Path) -> Found {
        Found::Nothing
    }
}

/// The programs that could open a file, with readable names, the system's helper programs left out, and the editors
/// and terminals that suit any file picked out.
pub fn choices(path: &Path) -> Choices {
    refine(platform::choices(path))
}

/// Every program on this computer that could be chosen, by name (Windows: the Start menu's programs; elsewhere the
/// programs the system lists), without helper programs, once each.
pub fn installed_apps(path: &Path) -> Vec<App> {
    let raw = platform::choices(path);
    let mut all = start_menu_apps();
    all.extend(extras());
    all.extend(raw.default);
    all.extend(raw.suggested);
    all.extend(raw.others);
    tidy_list(all)
}

// ---- Readable names, helpers left out, programs for any file ----------------------------------------------

/// Program names (lower case, without ".exe") and the names to show for them.
const FRIENDLY: &[(&str, &str)] = &[
    ("devenv", "Visual Studio"),
    ("code", "Visual Studio Code"),
    ("code - insiders", "Visual Studio Code Insiders"),
    ("vscodium", "VSCodium"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("zed", "Zed"),
    ("notepad++", "Notepad++"),
    ("notepad", "Notepad"),
    ("sublime_text", "Sublime Text"),
    ("subl", "Sublime Text"),
    ("powershell", "Windows PowerShell"),
    ("pwsh", "PowerShell 7"),
    ("cmd", "Command Prompt"),
    ("wt", "Windows Terminal"),
    ("msedge", "Microsoft Edge"),
    ("chrome", "Google Chrome"),
    ("firefox", "Firefox"),
    ("iexplore", "Internet Explorer"),
    ("wmplayer", "Windows Media Player"),
    ("mspaint", "Paint"),
    ("write", "WordPad"),
    ("wordpad", "WordPad"),
    ("winword", "Microsoft Word"),
    ("excel", "Microsoft Excel"),
    ("powerpnt", "Microsoft PowerPoint"),
    ("outlook", "Microsoft Outlook"),
    ("7zfm", "7-Zip File Manager"),
    ("acrobat", "Adobe Acrobat"),
    ("acrord32", "Adobe Acrobat Reader"),
    ("photoshop", "Adobe Photoshop"),
    ("illustrator", "Adobe Illustrator"),
    ("handbrake", "HandBrake"),
    ("vlc", "VLC media player"),
    ("subtitleedit", "Subtitle Edit"),
    ("recuva64", "Recuva"),
    ("recuva", "Recuva"),
    ("copilotapp", "Microsoft Copilot"),
];

fn stem_of(id: &str) -> String {
    let last = id.rsplit(['/', '\\']).next().unwrap_or(id);
    let lower = last.to_lowercase();
    lower.strip_suffix(".exe").or_else(|| lower.strip_suffix(".desktop")).unwrap_or(&lower).to_string()
}

/// The name to show for a program that is only known by its file name ("devenv" is Visual Studio).
pub fn friendly(name: &str, id: &str) -> Option<&'static str> {
    let stem = stem_of(id);
    if name.to_lowercase().trim_end_matches(".exe") != stem {
        return None; // it already has a proper name
    }
    FRIENDLY.iter().find(|(k, _)| *k == stem).map(|(_, v)| *v)
}

/// Programs that exist to serve other programs (installers, updaters, agents, shell extensions, diagnostics...) and
/// are no use for opening a file.
pub fn is_helper(name: &str, id: &str) -> bool {
    const WORDS: &[&str] = &["install", "unins", "update", "agent", "shellext", "diag", "licens", "helper", "packaging", "migrat", "crash", "setup", "regsvr", "bootstrap", "elevat", "notifier", "uninstall"];
    const EXACT: &[&str] = &["wab", "wabmig", "tabtip", "javaws", "acrodist", "acroinfo", "acrobatinfo", "cnmnsst", "iediagcmd", "scanutility", "iexplore", "conhost", "msiexec", "rundll32", "wermgr"];
    let stem = stem_of(id);
    let name = name.to_lowercase();
    EXACT.contains(&stem.as_str()) || EXACT.contains(&name.as_str()) || WORDS.iter().any(|w| name.contains(w) || stem.contains(w))
}

/// Editors first, then shells and terminals: what people open an unknown file with.
const GENERAL: &[&str] = &[
    "code", "visual studio code", "code - insiders", "vscodium", "cursor", "windsurf", "zed", "visual studio", "notepad++", "sublime", "sublime_text", "notepad", "gedit", "gnome-text-editor", "text editor", "kate", "kwrite", "mousepad", "xed", "pluma", "textedit",
    "geany", "idea", "pycharm", "webstorm", "clion", "rider", "goland", "windows terminal", "wt", "powershell 7", "pwsh", "windows powershell", "powershell", "command prompt", "cmd", "gnome-terminal", "konsole", "xfce4-terminal", "alacritty", "kitty", "iterm", "terminal", "xterm",
];

/// The programs from `pool` that suit any file, in the order of `GENERAL`, each once.
pub fn general_apps(pool: &[App]) -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    for keyword in GENERAL {
        let hit = pool.iter().find(|a| !is_helper(&a.name, &a.id) && (name_matches(&a.name, keyword) || name_matches(&stem_of(&a.id), keyword)));
        if let Some(app) = hit {
            if !out.iter().any(|o| o.id.eq_ignore_ascii_case(&app.id) || o.name.eq_ignore_ascii_case(&app.name)) {
                out.push(app.clone());
            }
        }
    }
    out.truncate(12);
    out
}

fn refine(mut c: Choices) -> Choices {
    let rename = |a: &mut App| {
        if let Some(n) = friendly(&a.name, &a.id) {
            a.name = n.to_string();
        }
    };
    c.default.iter_mut().for_each(rename);
    c.suggested.iter_mut().for_each(rename);
    c.others.iter_mut().for_each(rename);
    c.others.retain(|a| !is_helper(&a.name, &a.id));
    let mut pool: Vec<App> = extras();
    pool.extend(c.default.iter().cloned());
    pool.extend(c.suggested.iter().cloned());
    pool.extend(c.others.iter().cloned());
    c.general = general_apps(&pool);
    c
}

/// Readable names, no helpers, each program once (by file and by name), sorted by name.
pub fn tidy_list(all: Vec<App>) -> Vec<App> {
    let mut seen_id = std::collections::HashSet::new();
    let mut seen_name = std::collections::HashSet::new();
    let mut out: Vec<App> = Vec::new();
    for mut a in all {
        if let Some(n) = friendly(&a.name, &a.id) {
            a.name = n.to_string();
        }
        if is_helper(&a.name, &a.id) || !seen_id.insert(a.id.to_lowercase()) || !seen_name.insert(a.name.to_lowercase()) {
            continue;
        }
        out.push(a);
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// The Start menu's programs, from the JSON the script prints: `[{"n": "Visual Studio Code", "p": "C:\\...\\Code.exe"}]`.
pub fn parse_start_menu(json: &str) -> Vec<App> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json.trim().trim_start_matches('\u{feff}')) else { return vec![] };
    let items = match value {
        serde_json::Value::Array(a) => a,
        one @ serde_json::Value::Object(_) => vec![one],
        _ => vec![],
    };
    items
        .iter()
        .filter_map(|v| {
            let name = v.get("n")?.as_str()?.trim();
            let exe = v.get("p")?.as_str()?.trim();
            let lower = name.to_lowercase();
            let skip = ["uninstall", "readme", "release notes", "documentation", "website", "manual", "license", "help", "what's new"].iter().any(|w| lower.contains(w));
            (!name.is_empty() && !skip && exe.to_lowercase().ends_with(".exe")).then(|| App { id: exe.to_string(), name: name.to_string(), icon_url: None })
        })
        .collect()
}

#[cfg(windows)]
fn start_menu_apps() -> Vec<App> {
    use std::process::Command;
    const SCRIPT: &str = r#"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
$ErrorActionPreference='SilentlyContinue'
$sh=New-Object -ComObject WScript.Shell
$dirs=@("$env:ProgramData\Microsoft\Windows\Start Menu\Programs","$env:APPDATA\Microsoft\Windows\Start Menu\Programs")
$r=foreach($d in $dirs){ Get-ChildItem -LiteralPath $d -Recurse -Filter *.lnk | ForEach-Object { $t=$sh.CreateShortcut($_.FullName).TargetPath; if($t -like '*.exe'){ [pscustomobject]@{n=$_.BaseName;p=$t} } } }
ConvertTo-Json -InputObject @($r) -Compress"#;
    let out = crate::proc::quiet(Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])).output();
    match out {
        Ok(o) if o.status.success() => parse_start_menu(&String::from_utf8_lossy(&o.stdout)).into_iter().filter(|a| Path::new(&a.id).is_file()).collect(),
        _ => vec![],
    }
}

#[cfg(not(windows))]
fn start_menu_apps() -> Vec<App> {
    vec![]
}

/// Programs that are usually there on Windows and may not be registered anywhere a search finds them.
#[cfg(windows)]
fn extras() -> Vec<App> {
    let env = |k: &str| std::env::var(k).ok();
    let mut found: Vec<(String, &str)> = Vec::new();
    if let Some(sys) = env("SystemRoot") {
        found.push((format!(r"{sys}\System32\notepad.exe"), "Notepad"));
        found.push((format!(r"{sys}\System32\cmd.exe"), "Command Prompt"));
        found.push((format!(r"{sys}\System32\WindowsPowerShell\v1.0\powershell.exe"), "Windows PowerShell"));
        found.push((format!(r"{sys}\System32\mspaint.exe"), "Paint"));
    }
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(pf) = env(var) {
            found.push((format!(r"{pf}\PowerShell\7\pwsh.exe"), "PowerShell 7"));
            found.push((format!(r"{pf}\Notepad++\notepad++.exe"), "Notepad++"));
            found.push((format!(r"{pf}\Sublime Text\sublime_text.exe"), "Sublime Text"));
            found.push((format!(r"{pf}\Microsoft VS Code\Code.exe"), "Visual Studio Code"));
        }
    }
    if let Some(l) = env("LOCALAPPDATA") {
        found.push((format!(r"{l}\Programs\Microsoft VS Code\Code.exe"), "Visual Studio Code"));
        found.push((format!(r"{l}\Programs\Microsoft VS Code Insiders\Code - Insiders.exe"), "Visual Studio Code Insiders"));
        found.push((format!(r"{l}\Programs\VSCodium\VSCodium.exe"), "VSCodium"));
        found.push((format!(r"{l}\Programs\cursor\Cursor.exe"), "Cursor"));
        found.push((format!(r"{l}\Programs\Windsurf\Windsurf.exe"), "Windsurf"));
        found.push((format!(r"{l}\Microsoft\WindowsApps\wt.exe"), "Windows Terminal"));
    }
    found.into_iter().filter(|(p, _)| Path::new(p).is_file()).map(|(id, name)| App { id, name: name.to_string(), icon_url: None }).collect()
}

#[cfg(not(windows))]
fn extras() -> Vec<App> {
    vec![]
}

/// The system's own chooser window, where there is one (Windows only).
#[cfg(windows)]
pub fn system_chooser(path: &Path) -> Result<()> {
    platform::system_chooser(path)
}

#[cfg(not(windows))]
pub fn system_chooser(_path: &Path) -> Result<()> {
    Err(CoreError::Unsupported("this system has no app chooser of its own"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_shell() {
        if cfg!(unix) {
            assert_eq!(split_command(r#"vlc --one "a b" c\ d 'e f'"#), vec!["vlc", "--one", "a b", "c d", "e f"]);
        }
        assert_eq!(split_command(r#""C:\Program Files\x.exe" %1"#), vec![r"C:\Program Files\x.exe", "%1"]);
        assert_eq!(split_command(""), Vec::<String>::new());
        assert_eq!(split_command(r#"a "" b"#), vec!["a", "", "b"]);
    }

    #[test]
    fn only_a_program_that_is_really_gone_counts_as_missing() {
        assert!(names_missing_program(r#""C:\Definitely\Gone\player.exe" "%1""#));
        assert!(!names_missing_program("(value not set)"), "a Store app has no command to read");
        assert!(!names_missing_program(r#""%SystemRoot%\system32\mspaint.exe" "%1""#), "left to Windows");
        assert!(!names_missing_program(""));
        assert!(!names_missing_program("vlc.exe %1"), "found on PATH, not by this check");
    }

    #[test]
    fn extension_is_lowercase_with_dot() {
        assert_eq!(extension_of(Path::new("/x/Photo.JPG")), ".jpg");
        assert_eq!(extension_of(Path::new("/x/Makefile")), "");
    }

    fn app(id: &str, name: &str) -> App {
        App { id: id.into(), name: name.into(), icon_url: None }
    }

    #[test]
    fn picks_the_usual_program_for_a_file_type() {
        let choices = Choices {
            extension: ".png".into(),
            default: None,
            suggested: vec![],
            others: vec![app("C:\\Tools\\7zFM.exe", "7zFM"), app("C:\\Adobe\\Acrobat.exe", "Acrobat"), app("C:\\g\\chrome.exe", "chrome"), app("C:\\x\\i_view64.exe", "i_view64")],
            general: vec![],
        };
        // An image viewer beats the browser, and archivers and PDF readers are not offered.
        assert_eq!(likely_app(&choices).map(|a| a.name), Some("i_view64".into()));
        let only_junk = Choices { extension: ".png".into(), default: None, suggested: vec![], others: vec![app("a", "7zFM"), app("b", "Acrobat")], general: vec![] };
        assert_eq!(likely_app(&only_junk), None);
    }

    #[test]
    fn raw_program_names_become_readable_and_helpers_go() {
        assert_eq!(friendly("devenv", r"C:\VS\devenv.exe"), Some("Visual Studio"));
        assert_eq!(friendly("msedge", r"C:\E\msedge.exe"), Some("Microsoft Edge"));
        assert_eq!(friendly("Visual Studio Code", r"C:\x\Code.exe"), None);
        for junk in ["IEDIAGCMD", "licensemanagershellext", "ShellMcpServers.Packaging", "UnifiedPluginInstallerAgent", "wabmig", "TabTip", "CNMNSST", "SCANUTILITY", "javaws", "Acrodist", "AcrobatInfo"] {
            assert!(is_helper(junk, &format!(r"C:\x\{junk}.exe")), "{junk}");
        }
        for fine in ["Visual Studio Code", "Notepad++", "VLC media player", "HandBrake", "Adobe Photoshop", "Windows Terminal"] {
            assert!(!is_helper(fine, &format!(r"C:\x\{fine}.exe")), "{fine}");
        }
    }

    #[test]
    fn editors_and_terminals_are_picked_out_in_order() {
        let pool = vec![
            app(r"C:\x\vlc.exe", "VLC media player"),
            app(r"C:\s\cmd.exe", "Command Prompt"),
            app(r"C:\s\powershell.exe", "Windows PowerShell"),
            app(r"C:\s\notepad.exe", "Notepad"),
            app(r"C:\p\Code.exe", "Visual Studio Code"),
            app(r"C:\n\notepad++.exe", "Notepad++"),
            app(r"C:\v\vs_installer.exe", "Visual Studio Installer"),
        ];
        let names: Vec<String> = general_apps(&pool).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["Visual Studio Code", "Notepad++", "Notepad", "Windows PowerShell", "Command Prompt"]);
    }

    #[test]
    fn the_installed_list_is_clean_and_has_each_program_once() {
        let list = tidy_list(vec![
            app(r"C:\a\Code.exe", "Visual Studio Code"),
            app(r"C:\A\code.exe", "code"),
            app(r"C:\b\devenv.exe", "devenv"),
            app(r"C:\c\setup.exe", "setup"),
            app(r"C:\d\vlc.exe", "VLC media player"),
        ]);
        let names: Vec<&str> = list.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Visual Studio", "Visual Studio Code", "VLC media player"]);
    }

    #[test]
    fn reads_the_start_menu_script_output() {
        let json = "\u{feff}[{\"n\":\"Visual Studio Code\",\"p\":\"C:\\\\Code.exe\"},{\"n\":\"Uninstall X\",\"p\":\"C:\\\\u.exe\"},{\"n\":\"Docs\",\"p\":\"C:\\\\readme.txt\"}]";
        let apps = parse_start_menu(json);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Visual Studio Code");
        let one = parse_start_menu("{\"n\":\"HandBrake\",\"p\":\"C:\\\\hb.exe\"}");
        assert_eq!(one.len(), 1);
        assert!(parse_start_menu("").is_empty() && parse_start_menu("not json").is_empty() && parse_start_menu("[]").is_empty());
    }

    #[test]
    fn keywords_match_whole_words() {
        assert!(name_matches("Visual Studio Code", "code"));
        assert!(name_matches("code.exe", "code"));
        assert!(!name_matches("Barcode Scanner", "code"));
        assert!(name_matches("libreoffice-writer", "libreoffice"));
        assert!(name_matches("Notepad++", "notepad++"));
        assert!(!name_matches("Notepad++", "notepad") || name_matches("Notepad", "notepad"));
        assert!(name_matches("Eye of GNOME", "eye of gnome"));
    }

    #[test]
    fn the_system_suggestion_is_used_when_the_table_has_no_opinion() {
        let c = Choices { extension: ".weird".into(), default: None, suggested: vec![app("x.desktop", "Weird Viewer")], others: vec![app("y.desktop", "Other")], general: vec![] };
        assert_eq!(likely_app(&c).map(|a| a.name), Some("Weird Viewer".into()));
        let none = Choices { extension: "".into(), default: None, suggested: vec![app("x", "X")], others: vec![], general: vec![] };
        assert_eq!(likely_app(&none), None);
    }

    #[test]
    fn finds_programs_on_path() {
        assert!(program_exists(if cfg!(windows) { "cmd" } else { "sh" }));
        assert!(!program_exists("definitely-not-a-program-xyz"));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
    mod desktop {
        use super::super::platform::{command_for, parse_entry, program_of};
        use std::path::Path;

        const ENTRY: &str = "[Desktop Entry]\nType=Application\nName=Viewer\nName[de]=Anzeiger\nExec=env FOO=1 viewer --open %F\nIcon=viewer\nMimeType=image/png;image/jpeg;\n\n[Desktop Action x]\nName=Other\nExec=nothing\n";

        #[test]
        fn reads_the_main_group_only() {
            let e = parse_entry("viewer.desktop", ENTRY).unwrap();
            assert_eq!(e.name, "Viewer");
            assert_eq!(e.mimes, vec!["image/png", "image/jpeg"]);
            assert_eq!(program_of(&e.exec).as_deref(), Some("viewer"));
        }

        #[test]
        fn hidden_entries_are_skipped() {
            assert!(parse_entry("a.desktop", "[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n").is_none());
            assert!(parse_entry("a.desktop", "[Desktop Entry]\nType=Link\nName=A\nExec=a\n").is_none());
        }

        #[test]
        fn expands_field_codes() {
            let e = parse_entry("viewer.desktop", ENTRY).unwrap();
            let argv = command_for(&e, Path::new("/tmp/a b.png"));
            assert_eq!(argv, vec!["env", "FOO=1", "viewer", "--open", "/tmp/a b.png"]);
            let bare = parse_entry("b.desktop", "[Desktop Entry]\nType=Application\nName=B\nExec=b -x\n").unwrap();
            assert_eq!(command_for(&bare, Path::new("/f")), vec!["b", "-x", "/f"]);
        }
    }
}

#[cfg(test)]
mod opener_tests {
    use super::*;

    #[test]
    fn opener_words() {
        assert_eq!(Opener::of("app", "VLC media player", "vlc.desktop").text(), "VLC media player");
        assert_eq!(Opener::of("runs", "", "").text(), "Runs itself");
        assert_eq!(Opener::of("none", "", "").text(), "No program set");
        assert_eq!(Opener::of("missing", "Gone", "").text(), "Gone (missing)");
        assert_eq!(Opener::of("missing", "", "").text(), "Program missing");
    }

    #[cfg(unix)]
    #[test]
    fn a_runnable_file_without_an_extension_runs_itself() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("tool");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(default_opener(&tool, false).kind, "runs");
        let plain = dir.path().join("notes");
        std::fs::write(&plain, "x").unwrap();
        assert_ne!(default_opener(&plain, false).kind, "runs");
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
    mod unix {
        use super::super::defaults::unix::*;

        const GLOBS: &str = "# comment\n50:text/plain:*.txt\n50:image/png:*.png\n50:application/gzip:*.gz\n50:application/x-compressed-tar:*.tar.gz\n10:text/x-makefile:Makefile\n50:text/markdown:*.md\n50:image/jpeg:*.jpg:cs\n20:application/x-foo:*.f?o\n";

        #[test]
        fn names_become_types() {
            let g = parse_globs(GLOBS);
            assert_eq!(mime_for_name(&g, "notes.TXT").as_deref(), Some("text/plain"));
            assert_eq!(mime_for_name(&g, "Makefile").as_deref(), Some("text/x-makefile"));
            assert_eq!(mime_for_name(&g, "photo.JPG").as_deref(), Some("image/jpeg"));
            // The longest pattern wins over a shorter one that also fits.
            assert_eq!(mime_for_name(&g, "src.tar.gz").as_deref(), Some("application/x-compressed-tar"));
            assert_eq!(mime_for_name(&g, "file.gz").as_deref(), Some("application/gzip"));
            // Patterns with wildcards inside are not guessed at; a bare ".txt" is a name, not an extension.
            assert_eq!(mime_for_name(&g, "x.foo"), None);
            assert_eq!(mime_for_name(&g, ".txt"), None);
            assert_eq!(mime_for_name(&g, "README"), None);
        }

        #[test]
        fn defaults_come_from_the_most_specific_list_that_names_an_installed_app() {
            let user = parse_associations("[Default Applications]\nimage/png=gone.desktop;viewer.desktop;\n[Removed Associations]\ntext/plain=bad.desktop;\n");
            let system = parse_associations("[Default Applications]\nimage/png=other.desktop;\ntext/plain=bad.desktop;edit.desktop;\n[Added Associations]\nimage/jpeg=viewer.desktop;\n");
            let cache = parse_associations("[MIME Cache]\nimage/gif=gif.desktop;\ntext/plain=bad.desktop;\n");
            let have = |id: &str| matches!(id, "viewer.desktop" | "other.desktop" | "edit.desktop" | "bad.desktop" | "gif.desktop");
            let files = [user, system];
            let caches = [cache];
            // The user's own first choice is gone, so their next one is used before the system's.
            assert_eq!(pick("image/png", &files, &caches, &have).as_deref(), Some("viewer.desktop"));
            // Removed for this type by the user: skipped everywhere.
            assert_eq!(pick("text/plain", &files, &caches, &have).as_deref(), Some("edit.desktop"));
            // Added, and what the cache lists, count when no default is named.
            assert_eq!(pick("image/jpeg", &files, &caches, &have).as_deref(), Some("viewer.desktop"));
            assert_eq!(pick("image/gif", &files, &caches, &have).as_deref(), Some("gif.desktop"));
            assert_eq!(pick("video/mp4", &files, &caches, &have), None);
        }

        #[test]
        fn association_lists_parse() {
            let a = parse_associations("[Default Applications]\na/b=x.desktop;y.desktop;\n\n[Other]\na/b=z.desktop\n[Removed Associations]\na/b=q.desktop\n");
            assert_eq!(a.default["a/b"], vec!["x.desktop", "y.desktop"]);
            assert_eq!(a.removed["a/b"], vec!["q.desktop"]);
            assert!(a.added.is_empty());
        }
    }
}
