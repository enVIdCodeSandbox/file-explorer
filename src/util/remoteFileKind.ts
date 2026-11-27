/** Pure helpers for routing remote/SSH files between text editors (nano / VS Code) and byte copies. */

const BINARY_EXTS = new Set([
  "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "zst", "lz4", "cab", "wim", "arj", "lzh", "lha", "cpio",
  "iso", "img", "dmg", "vhd", "vhdx", "vmdk", "exe", "msi", "app", "apk", "deb", "rpm", "appimage", "pkg", "dll", "so", "dylib", "sys",
  "bin", "dat", "o", "a", "obj", "lib", "wasm", "pyc", "class", "jar",
  "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff", "heic", "heif", "avif", "psd", "ai", "xcf", "fig", "sketch",
  "mp4", "mkv", "mov", "avi", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "3gp", "3g2", "m2ts", "m2t", "m2v", "ogv", "ogm", "vob", "asf", "divx", "f4v", "rm", "rmvb", "mxf", "tod",
  "mp3", "flac", "wav", "ogg", "m4a", "aac", "opus", "wma", "aiff", "mid", "midi",
  "pdf", "doc", "docx", "docm", "odt", "pages", "xls", "xlsx", "xlsm", "ods", "numbers", "ppt", "pptx", "pps", "ppsx", "odp", "key", "epub", "mobi", "azw3",
  "db", "sqlite", "sqlite3", "mdb", "accdb", "ttf", "otf", "woff", "woff2", "fbx", "stl", "blend", "glb", "kdbx", "pfx", "gpg",
]);

const TEXT_EXTS = new Set([
  "txt", "md", "markdown", "rst", "adoc", "tex", "rtf", "csv", "tsv", "log", "out", "err",
  "js", "mjs", "cjs", "jsx", "ts", "tsx", "rs", "py", "go", "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "java", "kt", "kts", "swift",
  "cs", "vb", "php", "rb", "lua", "dart", "scala", "r", "jl", "zig", "hs", "ex", "exs", "erl", "clj", "sql",
  "html", "htm", "css", "scss", "sass", "less", "vue", "svelte", "astro", "sh", "bash", "zsh", "fish", "ksh", "csh",
  "bat", "cmd", "ps1", "awk", "sed", "pl", "pm", "patch", "diff", "graphql", "gql", "proto",
  "json", "jsonc", "json5", "yaml", "yml", "toml", "xml", "svg", "ini", "cfg", "conf", "cnf", "cf", "env", "properties", "props",
  "plist", "reg", "lock", "service", "socket", "timer", "target", "mount", "automount", "path", "slice", "scope", "rules", "list", "sources",
  "repo", "desktop", "gitignore", "gitattributes", "gitmodules", "editorconfig", "htaccess", "npmrc", "yarnrc",
  "bak", "old", "orig", "sample", "example", "dist", "default", "pem", "crt", "cer", "pub", "nix", "mk", "cmake", "gradle", "tf", "tfvars", "hcl", "dockerfile",
]);

const TEXT_NAMES = new Set([
  "makefile", "gnumakefile", "cmakelists.txt", "dockerfile", "containerfile", "vagrantfile", "procfile", "gemfile", "rakefile",
  "license", "copying", "notice", "authors", "changelog", "readme", "todo", "hosts", "hostname", "resolv.conf", "nsswitch.conf",
  "fstab", "mtab", "crypttab", "exports", "passwd", "passwd-", "group", "group-", "shadow", "shadow-", "gshadow", "gshadow-",
  "sudoers", "shells", "profile", "bash_profile", "bash_logout", "bashrc", "zshrc", "zprofile", "vimrc", "nanorc", "tmux.conf",
  "gitconfig", "authorized_keys", "known_hosts", "config", "issue", "motd", "os-release", "lsb-release", "crontab", "anacrontab", "inittab", "modules", "sysctl.conf",
]);

/** Fast name-based check; `"unknown"` means the caller should sniff leading bytes on the server. */
export function classifyRemoteFile(name: string): "text" | "binary" | "unknown" {
  const lower = (name.split("/").pop() ?? name).toLowerCase();
  if (!lower) return "unknown";
  if (lower.includes(".tar.")) return "binary";
  const dot = lower.lastIndexOf(".");
  if (dot >= 0 && dot < lower.length - 1) {
    const ext = lower.slice(dot + 1);
    if (BINARY_EXTS.has(ext)) return "binary";
    if (TEXT_EXTS.has(ext)) return "text";
  }
  if (TEXT_NAMES.has(lower)) return "text";
  return "unknown";
}

/** Single-quote an argument safely for POSIX shells (`sh`/`bash`/`zsh`). */
export function posixShellQuote(arg: string): string {
  return `'${arg.replace(/'/g, "'\\''")}'`;
}

/** Command run in the SSH PTY to edit `remotePath` in `nano`, escalating via `sudo` only if unreadable. */
export function sshNanoCommand(remotePath: string, forceSudo = false): string {
  const q = posixShellQuote(remotePath);
  if (forceSudo) return ` sudo nano -- ${q}\r`;
  return ` if [ -r ${q} ] || [ ! -e ${q} ]; then nano -- ${q}; else sudo nano -- ${q}; fi\r`;
}
