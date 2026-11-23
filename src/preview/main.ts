// A way to look at the interface in an ordinary browser, with a pretend device
// behind it instead of the real backend. Only `npm run dev` serves it; it is not
// part of the built app.
//
//   http://localhost:1420/preview.html                    a pretend Android phone
//   http://localhost:1420/preview.html?look=samsung       google | samsung | apple
//   http://localhost:1420/preview.html?look=apple&theme=light
//   http://localhost:1420/preview.html?os=windows         the computer layout
//   ...&setup=1 shows the first-run setup, &open=/storage/emulated/0/Download opens a folder
//
// Nothing is read from or written to this computer: files, storage and settings are made up.

import { emit } from "@tauri-apps/api/event";
import { columnDef } from "../util/columns";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AppInfo, Drive, FileEntry, Session, Settings } from "../api";
import { applyPatch, perfPresetsFor, recommendMobilePerf, UI_PRESETS } from "../settings/presets";
import defaults from "./defaults.json";

const q = new URLSearchParams(location.search);
const os = q.get("os") ?? (q.get("look") === "apple" ? "ios" : "android");
const mobile = os === "android" || os === "ios";
const look = q.get("look") ?? (mobile ? (os === "ios" ? "apple" : "google") : os);
const root = os === "ios" ? "/var/mobile/Containers/Data/Application/1234/Documents" : os === "windows" ? "C:\\Users\\me" : "/storage/emulated/0";
const sep = os === "windows" ? "\\" : "/";
const day = 86_400_000;
const fakeSessions = new Map<string, Record<string, unknown>>();
const fakeOutput = new Map<string, string>();
const now = Date.now();

type Node = { dir?: Record<string, Node>; size?: number; age?: number };
const file = (size: number, age = 3): Node => ({ size, age });
const tree: Node = {
  dir: {
    DCIM: { dir: { Camera: { dir: Object.fromEntries(Array.from({ length: 48 }, (_, i) => [`IMG_${20261100 + i}.jpg`, file(2_400_000 + i * 31_000, i / 3)])) } } },
    Documents: { dir: { "Lease agreement.pdf": file(482_133, 40), "Budget 2026.xlsx": file(88_412, 6), "Notes.txt": file(2_210, 1), Taxes: { dir: { "2025 return.pdf": file(1_203_881, 200) } } } },
    Download: { dir: { "fast-explorer.zip": file(18_402_113, 0.1), "boarding-pass.pdf": file(96_004, 2), "ringtone.mp3": file(3_911_002, 12), "holiday.mp4": file(248_001_774, 20), "setup.apk": file(54_100_221, 30) } },
    Movies: { dir: { "Trip to the coast.mp4": file(1_812_330_112, 90) } },
    Music: { dir: { "Morning mix.flac": file(31_220_881, 300), "Podcast episode 12.mp3": file(48_113_090, 9) } },
    Pictures: { dir: { Screenshots: { dir: { "Screenshot_20261113.png": file(412_008, 1), "Screenshot_20261112.png": file(388_771, 2) } }, "wallpaper.png": file(5_100_300, 60) } },
    ".nomedia": file(0, 400),
  },
};
const sd: Node = { dir: { Backups: { dir: { "phone-2026-10.tar.gz": file(4_800_000_000, 30) } }, "readme.txt": file(120, 500) } };

const drives: Drive[] =
  os === "windows"
    ? [{ name: "Local Disk", mount: "C:\\", fs: "NTFS", total: 512e9, available: 190e9, media: "ssd", removable: false, readOnly: false, online: true }]
    : os === "ios"
      ? [{ name: "On this device", mount: root, fs: "", total: 256e9, available: 91e9, media: "ssd", removable: false, readOnly: false, online: true }]
      : [
          { name: "Internal storage", mount: root, fs: "", total: 256e9, available: 61e9, media: "ssd", removable: false, readOnly: false, online: true },
          { name: "SD card", mount: "/storage/1A2B-3C4D", fs: "", total: 128e9, available: 12e9, media: "removable", removable: true, readOnly: false, online: true },
        ];

function find(path: string): Node | null {
  const base = path.startsWith("/storage/1A2B-3C4D") ? { node: sd, at: "/storage/1A2B-3C4D" } : path.startsWith(root) ? { node: tree, at: root } : os === "windows" && path === "C:\\" ? { node: { dir: { Users: { dir: { me: tree } } } } as Node, at: "C:\\" } : null;
  if (!base) return null;
  let node: Node | undefined = base.node;
  for (const part of path.slice(base.at.length).split(/[\\/]/).filter(Boolean)) node = node?.dir?.[part];
  return node ?? null;
}

const entriesOf = (node: Node): FileEntry[] =>
  Object.entries(node.dir ?? {}).map(([name, n]) => ({
    name,
    isDir: !!n.dir,
    isSymlink: false,
    isHidden: name.startsWith("."),
    size: n.size ?? 0,
    modified: now - (n.age ?? 5) * day,
    created: now - (n.age ?? 5) * day,
  }));

let settings = structuredClone(defaults) as unknown as Settings;
const system = mobile ? { cpus: 8, totalRamMb: 7400, availableRamMb: 2300, onBattery: true, batteryPercent: 83 } : { cpus: 16, totalRamMb: 32_000, availableRamMb: 20_000, onBattery: false, batteryPercent: null };
settings.setupDone = q.get("setup") !== "1";
applyPatch(settings, UI_PRESETS.find((p) => p.id === look)?.patch ?? {});
applyPatch(settings, UI_PRESETS.find((p) => p.id === look)?.start ?? {});
if (mobile) applyPatch(settings, (perfPresetsFor(true).find((p) => p.id === recommendMobilePerf(system.totalRamMb, system.cpus).id) ?? perfPresetsFor(true)[0]).patch);
const preset = UI_PRESETS.find((p) => p.id === look);
if (preset) settings.appearance.theme = q.get("theme") === "light" ? preset.themes.light : preset.themes.dark;
if (q.get("view")) settings.appearance.viewLevel = Number(q.get("view"));

const places = [
  { label: "Home", path: root },
  ...["Download:Downloads", "Documents:Documents", "Pictures:Pictures", "DCIM:Camera", "Music:Music", "Movies:Videos"].map((x) => ({ label: x.split(":")[1], path: `${root}${sep}${x.split(":")[0]}` })),
];
const open = q.get("open");
const session: Session = {
  tabs: open ? [{ id: "t1", location: open, history: [open], historyIndex: 0, groupId: null }] : [],
  groups: [],
  activeTab: open ? "t1" : null,
  pins: [{ path: `${root}${sep}Documents${sep}Taxes`, label: "Taxes" }] as Session["pins"],
  recent: [
    { path: `${root}${sep}Download${sep}fast-explorer.zip`, isDir: false, at: now - 3_600_000 },
    { path: `${root}${sep}Documents${sep}Budget 2026.xlsx`, isDir: false, at: now - day },
    { path: `${root}${sep}DCIM${sep}Camera`, isDir: true, at: now - 2 * day },
    { path: `${root}${sep}Documents${sep}Notes.txt`, isDir: false, at: now - 3 * day },
  ] as Session["recent"],
  drives,
  searches: [],
};
const info: AppInfo = {
  os,
  version: __APP_VERSION__,
  separator: sep,
  home: root,
  budget: { tier: "balanced", ioThreads: 2, indexThreads: 1, thumbThreads: 1, slowThumbThreads: 1, cacheMb: 64, indexMaxEntries: 100_000, listingCacheDirs: 32, backgroundIndexing: false, prefetch: false, thumbnails: false, prewarmLimit: 64, prewarmImages: false, prefetchLimit: 2, folderFillLimit: 48, dirBatchMax: 512, intensity: 45, background: false },
  system,
  index: { state: "idle", entries: 0, memoryBytes: 0, builtAt: 0, truncated: false, roots: [], drivesReady: 0, drivesTotal: 0 },
  thumbnailTypes: [],
};

let fm = { supported: true, active: false, previous: null as string | null, detail: "Folders opened from other apps go to File Explorer.", limits: ["\"Show in folder\" in browsers and many apps asks File Explorer directly and still opens there."] };


const installedPacks = new Set<string>();
const PACK_CATALOG = [
  ["lucide", "Lucide", "clean outlines", 674000],
  ["feather", "Feather", "simple outlines, small", 72000],
  ["mdi", "Material Design", "Google's icons, very many", 3100000],
];
function fakePacks() {
  const packs = PACK_CATALOG.map(([id, label, about, download]) => ({ id, label, about, download, installed: installedPacks.has(String(id)), disk: installedPacks.has(String(id)) ? Number(download) : 0 }));
  return { packs, disk: packs.reduce((n, p) => n + p.disk, 0) };
}
function fakePackJson(query: string) {
  const all: Record<string, string> = {
    home: '<path fill="none" stroke="currentColor" stroke-width="2" d="M3 11l9-8l9 8v10H3z"/>',
    heart: '<path fill="none" stroke="currentColor" stroke-width="2" d="M12 21C5 15 3 11 3 8a4.5 4.5 0 0 1 9-1a4.5 4.5 0 0 1 9 1c0 3-2 7-9 13z"/>',
    "hard-drive": '<path fill="none" stroke="currentColor" stroke-width="2" d="M3 14h18v6H3zM5 14l3-9h8l3 9"/>',
    star: '<path fill="none" stroke="currentColor" stroke-width="2" d="M12 3l3 6l6 1l-4.500 4.500l1 6.500L12 18l-5.500 3l1-6.500L3 10l6-1z"/>',
  };
  const icons = Object.fromEntries(Object.entries(all).filter(([n]) => !query || n.includes(query)));
  return JSON.stringify({ w: 24, h: 24, icons });
}

const answers: Record<string, (a: Record<string, unknown>) => unknown> = {
  get_settings: () => settings,
  set_settings: (a) => void (settings = a.settings as Settings),
  get_session: () => session,
  take_launch: () => [],
  smart_clear_cancel: () => null,
  smart_clear_scan: () => {
    const day = 86_400;
    const now = Math.floor(Date.now() / 1000);
    const it = (name: string, dir: string, kind: string, mb: number, days: number, isDir = false, note = "") => ({ path: `${dir}/${name}`, also: [], name, size: Math.round(mb * 1024 * 1024), used: now - days * day, kind, isDir, group: null, note });
    const items = [
      it("pip-build-x81", "/tmp", "temp", 412, 12, true),
      it("~$Budget.xlsx", "/home/me/Documents", "temp", 0.2, 40),
      it("google-chrome", "/home/me/.cache", "cache", 1840, 1, true),
      it("spotify", "/home/me/.cache", "cache", 920, 2, true),
      it("mesa_shader_cache", "/home/me/.cache", "cache", 310, 5, true),
      it("thumbnails", "/home/me/.cache", "thumbnails", 265, 1, true),
      it("old-project.zip", "/home/me/.local/share/Trash/files", "trash", 730, 20),
      it("ubuntu-24.04.iso.crdownload", "/home/me/Downloads", "partial", 2100, 75),
      it("app-debug.log", "/home/me/Projects/site", "logs", 88, 200),
      it("ubuntu-24.04-desktop.iso", "/home/me/Downloads", "installers", 5900, 140),
      it("GIMP-3.0.AppImage", "/home/me/Downloads", "installers", 230, 300),
      it("node_modules", "/home/me/Projects/old-site", "buildLeftovers", 640, 500, true),
      it("holiday (1).mp4", "/home/me/Downloads", "duplicates", 1400, 220, false, "Same contents as /home/me/Videos/holiday.mp4"),
      it("lecture-recordings.zip", "/home/me/Downloads", "oldDownloads", 3200, 420),
      it("vm-backup.qcow2", "/home/me/Documents", "largeOld", 18400, 610),
    ];
    const totals = Object.values(items.reduce<Record<string, { kind: string; count: number; bytes: number; listed: number }>>((acc, i) => {
      const t = (acc[i.kind] ??= { kind: i.kind, count: 0, bytes: 0, listed: 0 });
      t.count++; t.listed++; t.bytes += i.size;
      return acc;
    }, {}));
    return { items, totals, scanned: 184_203, cancelled: false, lookedIn: ["/home/me"] };
  },
  default_fm_status: () => fm,
  default_fm_set: (a) => (fm = { ...fm, active: Boolean(a.on), detail: a.on ? "Folders opened from other apps open here." : "Folders opened from other apps go to File Explorer." }),
  trash_list: () => ({
    canRestore: true,
    label: "Recycle Bin",
    items: [
      { id: "t1", name: "old-render.mp4", originalParent: "C:\\Users\\me\\Videos", deleted: Math.floor(Date.now() / 1000) - 3600, size: 482_000_000, entries: null, isDir: false, path: null },
      { id: "t2", name: "Budget 2025.xlsx", originalParent: "C:\\Users\\me\\Documents", deleted: Math.floor(Date.now() / 1000) - 86400 * 2, size: 48_200, entries: null, isDir: false, path: null },
      { id: "t3", name: "node_modules", originalParent: "C:\\Projects\\site", deleted: Math.floor(Date.now() / 1000) - 86400 * 9, size: null, entries: 214, isDir: true, path: null },
    ],
  }),
  list_fonts: () => [
    { name: "Arial", fixedWidth: false },
    { name: "Consolas", fixedWidth: true },
    { name: "DejaVu Serif", fixedWidth: false },
  ],
  default_app: (a) => {
    const name = String(a.path).split(/[\\/]/).pop() ?? "";
    const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
    const apps: Record<string, string> = { txt: "Notepad", md: "Visual Studio Code", png: "Photos", jpg: "Photos", mp4: "VLC media player", pdf: "Microsoft Edge", docx: "Word" };
    if (ext === "exe" || ext === "sh") return { kind: "runs", name: "", id: "", iconUrl: null };
    if (apps[ext]) return { kind: "app", name: apps[ext], id: `C:\\Apps\\${apps[ext]}.exe`, iconUrl: null };
    return { kind: "none", name: "", id: "", iconUrl: null };
  },
  open_choices: (a) => ({
    extension: String(a.path).includes(".") ? "." + String(a.path).split(".").pop() : "",
    default: null,
    suggested: [],
    others: [],
    general: [
      { id: "C:\\Code.exe", name: "Visual Studio Code", iconUrl: null },
      { id: "C:\\notepad++.exe", name: "Notepad++", iconUrl: null },
      { id: "C:\\notepad.exe", name: "Notepad", iconUrl: null },
      { id: "C:\\wt.exe", name: "Windows Terminal", iconUrl: null },
      { id: "C:\\powershell.exe", name: "Windows PowerShell", iconUrl: null },
      { id: "C:\\cmd.exe", name: "Command Prompt", iconUrl: null },
    ],
  }),
  installed_apps: () => ["Adobe Acrobat", "Command Prompt", "Google Chrome", "HandBrake", "Microsoft Edge", "Notepad", "VLC media player", "Visual Studio Code", "Windows PowerShell"].map((name) => ({ id: `C:\\${name}.exe`, name, iconUrl: null })),
  open_with_app: () => null,
  rename_item: (a) => String(a.path).replace(/[^\\/]+$/, String(a.newName)),
  trash_restore: () => 1,
  trash_purge: () => 1,
  trash_empty: () => 3,
  app_info: () => info,
  get_drives: () => drives,
  known_folders: () => places,
  list_dir: (a) => {
    const node = find(String(a.path));
    if (!node?.dir) throw new Error(`not found: ${a.path}`);
    return { path: a.path, parent: null, entries: entriesOf(node), cached: false };
  },
  search: (a) => ({ id: a.id, hits: [], total: 0, truncated: false, indexed: false, tookMs: 1 }),
  locks_list: () => ({ locks: [], hasMain: false, unlocked: [] }),
  history_info: () => ({ entries: 0, bytes: 0 }),
  transfer_methods: () => ({ installed: ["native", "robocopy", "smb", "rclone"], stats: [{ method: "native", uses: 14, avgBytesPerSec: 210e6 }, { method: "robocopy", uses: 6, avgBytesPerSec: 340e6 }], jobs: [{ kind: "delete", uses: 9, avgItemsPerSec: 4200 }, { kind: "trash", uses: 12, avgItemsPerSec: 6.5 }, { kind: "extract", uses: 2, avgItemsPerSec: 800 }], instantMoves: 31 }),
  column_values: (a) =>
    (a.names as string[]).map((n, i) => ({
      name: n,
      values: Object.fromEntries(
        (a.keys as string[]).flatMap((k) => {
          const d = columnDef(k);
          if (!d || i % 7 === 6) return []; // every seventh item has nothing to say, as real files often do
          if (d.kind === "date") return [[k, String(1.7e12 - i * 5e8)]];
          if (d.kind === "size") return [[k, String(1000 * (i + 1) ** 3)]];
          if (d.kind === "number") return [[k, String((i * 37) % 11 + 1)]];
          const choices = ["Canon EOS R5", "Nikon Z6", "Sony A7", "-rw-r--r--", "-rwxr-xr-x"];
          if (k === "openWith") return [[k, ["Notepad", "Photos", "VLC media player", "Runs itself", "No program set"][i % 5]]];
          return [[k, k === "mime" ? ["image/jpeg", "text/plain"][i % 2] : k.startsWith("sha") || k === "md5" ? "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".slice(0, k === "md5" ? 32 : 40) : `${choices[i % choices.length]}`]];
        }),
      ),
    })),
  // A file that grows by about 3 MB a second, so the live panel can be seen.
  stat_many: (a) => (a.paths as string[]).map((path) => ({ path, exists: true, size: /\.mp4$/i.test(path) ? 465_000_000 + Math.floor((Date.now() % 1e7) / 1000) * 3_000_000 : 88_000, modified: Date.now() })),
  get_details: (a) => {
    const path = String(a.path);
    const name = path.split(/[\\/]/).pop() ?? path;
    const isDir = !/\.[a-z0-9]+$/i.test(name);
    return { name, path, parent: path.slice(0, Math.max(0, path.length - name.length - 1)), kind: isDir ? "folder" : "file", size: isDir ? 0 : 88_000, sizeOnDisk: isDir ? null : 90_112, created: now - 40 * day, modified: now - 2 * day, accessed: now - day, readonly: false, hidden: false, extension: isDir ? null : name.split(".").pop() ?? null, mime: isDir ? null : "application/octet-stream", symlinkTarget: null, unix: null, attributes: ["Archive"], image: null, itemCount: isDir ? 12 : null, volume: { name: "Local Disk", mount: "C:\\", fs: "NTFS", total: 512e9, available: 190e9, media: "ssd", removable: false, readOnly: false, online: true }, media: [] };
  },
  folder_stats: () => ({ files: 403, folders: 97, totalBytes: 9.66e9, maxDepth: 6, largest: [], byType: [{ extension: "zip", count: 79, bytes: 5.16e9 }, { extension: "mp4", count: 8, bytes: 2.15e9 }], newest: null, oldest: null, emptyFolders: 3, complete: true }),
  thumbs_prewarm: () => null,
  crash_logs: () => [{ name: "crash-2026-11-15-19-40-00-1.txt", bytes: 5120, modified: 1763235600, sent: false }],
  crash_log_read: () => "=== FAST File Explorer crash report ===\nTime (UTC): 2026-11-16 04:12:09\nKind: Panic\n\n--- Computer ---\nOS: Windows 11\n\n--- What happened last ---\n04:12:08.100 click button \"Copy\"\n04:12:08.900 -> drag_out",
  crash_previous: () => null,
  crash_folder: () => "C:\\Users\\me\\AppData\\Roaming\\FAST\\crash-logs",
  crash_open_folder: () => null,
  crash_note: () => null,
  crash_context: () => null,
  folder_fill: (a) => (a.paths as string[]).map((_, i) => (i % 3 === 2 ? "empty" : "files")),
  tool_cancel: () => null,
  tool_run: async (a) => {
    const spec = a.spec as { kind: string };
    const id = Number(a.id);
    await emit("tool-progress", { id, done: 1, total: 2, note: "working" });
    switch (spec.kind) {
      case "compareFiles":
        return { result: "text", changed: 1, added: 1, removed: 0, truncated: false, rows: [
          { kind: "same", aNo: 1, a: "alpha", bNo: 1, b: "alpha" },
          { kind: "changed", aNo: 2, a: "beta", bNo: 2, b: "BETA!" },
          { kind: "added", aNo: null, a: "", bNo: 3, b: "gamma" } ] };
      case "compareDirs":
        return { same: 5, truncated: false, entries: [
          { rel: "a.txt", status: "leftOnly", left: { size: 10, modified: 1, isDir: false }, right: null },
          { rel: "sub\\b.txt", status: "rightNewer", left: { size: 10, modified: 1, isDir: false }, right: { size: 12, modified: 5, isDir: false } } ] };
      case "duplicates":
        return [{ size: 2048, files: [{ path: "C:\\x\\a.bin", modified: 1000 }, { path: "C:\\x\\copy of a.bin", modified: 2000 }] }];
      case "grep":
        await emit("tool-hits", { id, hits: [{ path: "C:\\x\\notes.txt", entry: null, line: 3, text: "the needle is here" }, { path: "C:\\x\\doc.docx", entry: "word/document.xml", line: 1, text: "needle again" }] });
        return { files: 12, hits: 2, truncated: false };
      case "split": return ["C:\\x\\big.001", "C:\\x\\big.002", "C:\\x\\big.crc"];
      case "join": return { path: "C:\\x\\big", verified: true };
      case "encode": return "C:\\x\\big.uue";
      case "decode": return ["C:\\x\\big"];
      case "convert": return [{ path: "C:\\x\\p.png", out: "C:\\x\\p.jpg", error: null }];
    }
    return null;
  },
  view_info: (a) => ({ name: String(a.path).split(/[\\/]/).pop(), size: /\.png$/i.test(String(a.path)) ? 70 : 1_000_000 * 40, modified: now }),
  view_read: (a) => {
    const offset = Number(a.offset);
    const len = Number(a.len);
    if (/\.png$/i.test(String(a.path))) return "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    // 40 bytes per line: "Line 123456 xxxxx…" and a line feed.
    let out = "";
    const first = Math.floor(offset / 40);
    for (let k = first; out.length < len + 40 && k < 1_000_000; k++) out += `Line ${k} `.padEnd(39, "x") + "\n";
    const text = out.slice(offset - first * 40, offset - first * 40 + len);
    return btoa(text);
  },
  view_find: (a) => {
    const needle = atob(String(a.needle));
    const m = /^line (\d+)/i.exec(needle);
    return m ? Number(m[1]) * 40 : null;
  },
  archive_stage: (a) => a.sources,
  run_program: (a) => { ((window as unknown as { __ran?: unknown[] }).__ran ??= []).push(a); return null; },
  // Like the real thing: remembered sizes go out the moment they are asked for.
  folder_sizes_start: (a) => {
    (a.names as string[]).forEach((name, i) => void emit("folder-size", { dir: a.dir, name, bytes: (i + 1) * 1_000_000, files: i + 1, complete: true, cached: true }));
  },
  save_text_file: () => null,
  term_info: () => ({ appElevated: false, os, windowsBuild: os === "windows" ? 22621 : 0, defaultShell: os === "windows" ? "pwsh" : "bash" }),
  term_list: () => [...fakeSessions.values()],
  term_spawn: (a) => {
    const sid = `s${fakeSessions.size + 1}`;
    fakeSessions.set(sid, { sid, title: os === "windows" ? "pwsh" : "bash", cwd: String(a.cwd), program: "sh", cols: 80, rows: 24, alive: true, exit: null, elevated: !!a.elevated, created: Date.now(), bytes: 0 });
    fakeOutput.set(sid, `\x1b[32m${String(a.cwd)}\x1b[0m $ `);
    return sid;
  },
  term_attach: (a) => ({ data: btoa(fakeOutput.get(String(a.sid)) ?? ""), info: fakeSessions.get(String(a.sid)) }),
  term_input: (a) => {
    const sid = String(a.sid);
    const typed = atob(String(a.data));
    const line = typed === "\r" ? "\r\n\x1b[36mhello from the preview\x1b[0m\r\n$ " : typed;
    fakeOutput.set(sid, (fakeOutput.get(sid) ?? "") + line);
    void emit("term-out", { sid, data: btoa(line) });
    return null;
  },
  term_resize: () => null,
  term_do: (a) => { if (a.action === "remove") fakeSessions.delete(String(a.sid)); if (a.action === "clear") fakeOutput.set(String(a.sid), ""); return null; },
  term_transcript: () => null,
  term_sweep_transcripts: () => null,
  // Icon collections: nothing is installed to begin with; a preview and an install work on a few made-up icons.
  icon_packs_list: () => fakePacks(),
  icon_pack_read: (a) => {
    if (!installedPacks.has(String(a.id))) throw new Error("That icon collection is not installed.");
    return fakePackJson("");
  },
  icon_pack_preview: (a) => {
    const q = String(a.query ?? "").trim().toLowerCase();
    const json = fakePackJson(q);
    return { pack: json, total: Object.keys(JSON.parse(json).icons).length };
  },
  icon_packs_search: async (a) => {
    await new Promise((r) => setTimeout(r, 250));
    const q = String(a.query ?? "").trim().toLowerCase();
    const json = fakePackJson(q);
    const total = Object.keys(JSON.parse(json).icons).length;
    return { hits: total ? (a.ids as string[]).map((id, i) => ({ id, total: total + i, pack: json })) : [], failed: null };
  },
  icon_pack_install: async (a) => {
    const id = String(a.id);
    await emit("icon-pack-progress", { id, phase: "connecting", done: 0, total: 674000 });
    await new Promise((r) => setTimeout(r, 500));
    for (let i = 1; i <= 8; i++) {
      await emit("icon-pack-progress", { id, phase: "downloading", done: i * 80000, total: 674000 });
      await new Promise((r) => setTimeout(r, (window as unknown as { __slow?: number }).__slow ?? 60));
    }
    await emit("icon-pack-progress", { id, phase: "saving", done: 674000, total: 674000 });
    installedPacks.add(String(a.id));
    return fakePacks();
  },
  icon_pack_remove: (a) => {
    installedPacks.delete(String(a.id));
    return fakePacks();
  },
  icon_packs_clear: () => {
    installedPacks.clear();
    return fakePacks();
  },
  clipboard_set_files: (a) => { ((window as unknown as { __clip?: unknown[] }).__clip ??= []).push(a.paths); return null; },
  clipboard_clear: () => { ((window as unknown as { __clip?: unknown[] }).__clip ??= []).push("cleared"); return null; },
  remote_stage_start: () => {
    setTimeout(() => void emit("delete-progress", { id: 900, phase: "done", permanent: false, selected: 1, itemsTotal: 1, itemsDone: 1, current: "", errors: [], errorCount: 0, elapsedSecs: 1, firstName: "a.txt", action: "stage", result: "/tmp/clip-1/0/a.txt", bytesDone: 1, bytesTotal: 1 }), 50);
    return 900;
  },
  clipboard_get: () => ({ paths: [], cut: false, virtualFiles: false }),
  check_conflicts: () => [],
  resource_usage: () => ({ cpuPercent: 3, memoryMb: 120, cores: system.cpus, totalRamMb: system.totalRamMb }),
  thumbnail: () => {
    throw new Error("no thumbnails in the preview");
  },
};

// Pretend automated tasks, so the Tasks window can be looked at.
const hour = 3_600_000;
const sampleTasks = [
  { id: "t1", name: "Nightly document backup", notes: "Zips Documents to the NAS", enabled: true, triggers: [{ type: "daily", time: "02:30", days: [] }], actions: [{ type: "compress", from: ["C:\\Users\\me\\Documents"], to: "Z:\\Backups\\docs-{date}.zip", filter: { include: "", exclude: "*.tmp", olderThanDays: null, newerThanDays: null, minBytes: null, maxBytes: null, hidden: false, recursive: true } }], options: { onSleep: "resume", catchUp: true, retries: 2, retryDelaySecs: 5, timeoutMinutes: 0, stopOnError: true, onlyOnAc: false, runAsAdmin: false, notify: "failure", skipIfRunning: true, lowPriority: true }, created: now - 9 * day, updated: now - day, resumePending: false, needsAdmin: false },
  { id: "t2", name: "Clear old downloads", notes: "", enabled: true, triggers: [{ type: "every", minutes: 1440, anchor: now - 3 * day }, { type: "startup" }], actions: [{ type: "delete", paths: ["C:\\Users\\me\\Downloads"], filter: { include: "", exclude: "", olderThanDays: 30, newerThanDays: null, minBytes: null, maxBytes: null, hidden: false, recursive: true }, permanent: false, removeEmptyFolders: true, removeFolder: false }], options: { onSleep: "cancel", catchUp: true, retries: 0, retryDelaySecs: 5, timeoutMinutes: 30, stopOnError: false, onlyOnAc: true, runAsAdmin: false, notify: "always", skipIfRunning: true, lowPriority: true }, created: now - 20 * day, updated: now - 2 * day, resumePending: false, needsAdmin: true },
  { id: "t3", name: "Photos off the card", notes: "", enabled: false, triggers: [{ type: "driveConnected", name: "SD" }], actions: [{ type: "move", from: ["E:\\DCIM"], to: "C:\\Users\\me\\Pictures\\Import", filter: { include: "*.jpg; *.raw", exclude: "", olderThanDays: null, newerThanDays: null, minBytes: null, maxBytes: null, hidden: false, recursive: true }, conflict: "keepBoth", keepStructure: false }], options: { onSleep: "resume", catchUp: false, retries: 0, retryDelaySecs: 5, timeoutMinutes: 0, stopOnError: true, onlyOnAc: false, runAsAdmin: false, notify: "failure", skipIfRunning: true, lowPriority: true }, created: now - 40 * day, updated: now - 40 * day, resumePending: false, needsAdmin: false },
];
const sampleRuns = [0, 1, 2, 3, 4].map((i) => ({ taskId: "t1", started: now - (i + 1) * day, ended: now - (i + 1) * day + (95 + i * 9) * 1000, outcome: i === 3 ? "failed" : "success", reason: i === 0 ? "scheduled" : "catch-up (it was due while the computer was off or asleep)", items: 1, bytes: 1_450_000_000 - i * 40_000_000, errors: i === 3 ? ["Z:\\Backups\\docs-2026-11-11.zip: The network path was not found."] : [], attempts: 1, interruptedBy: null, elevated: false, name: "Nightly document backup" }));
// Calls that return a list; everything else unknown answers null.
const lists = /^(servers_list|get_devices|frequent_folders|record_visit|shell_menu|list_assets|share_list|cache_usage|clipboard_get_files)$/;

Object.assign(answers, {
  update_status: () => ({ status: "current", version: "0.2.0", sha: "a1b2c3d4e5f6", latest: null, ready: null, message: "", tokenSet: false, lastCheck: 0 }),
  update_check: () => ({ status: "available", version: "0.2.0", sha: "a1b2c3d4e5f6", latest: { source: "release", version: "0.3.0", runId: 1, artifactId: 2, sha: "f6e5d4c3b2a1", branch: "main", createdAt: new Date(now - 3600_000).toISOString(), size: 61_000_000 }, ready: null, message: "A newer build is available.", tokenSet: false, lastCheck: Math.floor(Date.now() / 1000) }),
  tasks_list: () => ({ tasks: sampleTasks, running: [], next: { t1: now + 5 * hour, t2: now + 20 * hour }, last: { t1: sampleRuns[0] }, enabled: true }),
  tasks_history: () => sampleRuns,
  tasks_stats: () => ({
    stats: { runs: 5, successes: 4, partial: 0, failures: 1, cancelled: 0, interrupted: 0, skipped: 0, successRate: 0.8, avgSecs: 104, medianSecs: 104, p90Secs: 122, minSecs: 95, maxSecs: 122, avgItems: 1, avgBytes: 1_390_000_000, avgBytesPerSec: 13_400_000, totalItems: 5, totalBytes: 6_950_000_000, lastRun: now - day, lastSuccess: now - day, lastFailure: now - 4 * day, lastError: "Z:\\Backups\\docs-2026-11-11.zip: The network path was not found.", speedTrend: 1.2 },
    estimate: { secs: 104, samples: 4, basis: "speed" },
  }),
  tasks_preview: () => ({ ops: [{ action: 0, kind: "zip", from: "412 files", to: "Z:\\Backups\\docs-2026-11-15.zip", bytes: 1_400_000_000 }], files: 1, bytes: 1_400_000_000, truncated: false, problems: [] }),
});

mockWindows("main");
// `&lag=300` makes every answer take that many milliseconds, as a busy or slow backend would.
const lag = Number(q.get("lag") ?? 0);
const later = <T,>(v: T): T | Promise<T> => (lag > 0 ? new Promise<T>((r) => setTimeout(() => r(v), lag)) : v);
mockIPC(
  (cmd, payload) => {
    const a = (payload ?? {}) as Record<string, unknown>;
    if (cmd in answers) return later(answers[cmd](a));
    if (lists.test(cmd)) return later([]);
    if (cmd.startsWith("plugin:")) return null;
    return null;
  },
  { shouldMockEvents: true },
);

void import("../main");
// Lets a test script open dialogs and screens without clicking through them.
void import("../store").then((m) => ((window as unknown as { __store: unknown }).__store = m.useStore));
