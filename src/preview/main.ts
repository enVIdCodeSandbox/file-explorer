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
  version: "preview",
  separator: sep,
  home: root,
  budget: { tier: "balanced", ioThreads: 2, indexThreads: 1, thumbThreads: 1, cacheMb: 64, indexMaxEntries: 100_000, listingCacheDirs: 32, backgroundIndexing: false, prefetch: false, thumbnails: false },
  system,
  index: { state: "idle", entries: 0, memoryBytes: 0, builtAt: 0, truncated: false, roots: [], drivesReady: 0, drivesTotal: 0 },
  thumbnailTypes: [],
};

const answers: Record<string, (a: Record<string, unknown>) => unknown> = {
  get_settings: () => settings,
  set_settings: (a) => void (settings = a.settings as Settings),
  get_session: () => session,
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
  clipboard_get: () => ({ paths: [], cut: false, virtualFiles: false }),
  check_conflicts: () => [],
  resource_usage: () => ({ cpuPercent: 3, memoryMb: 120, cores: system.cpus, totalRamMb: system.totalRamMb }),
  thumbnail: () => {
    throw new Error("no thumbnails in the preview");
  },
};
// Calls that return a list; everything else unknown answers null.
const lists = /^(servers_list|get_devices|frequent_folders|record_visit|shell_menu|list_assets|share_list|cache_usage|clipboard_get_files)$/;

mockWindows("main");
mockIPC(
  (cmd, payload) => {
    const a = (payload ?? {}) as Record<string, unknown>;
    if (cmd in answers) return answers[cmd](a);
    if (lists.test(cmd)) return [];
    if (cmd.startsWith("plugin:")) return null;
    return null;
  },
  { shouldMockEvents: true },
);

void import("../main");
