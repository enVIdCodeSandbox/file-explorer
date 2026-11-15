// Presets and profiles.
//
//  - Performance presets: twelve steps from "Barebones" to "God-like" on a
//    computer, and six much lighter steps on a phone or tablet. Each one is a
//    set of ordinary settings (threads, cache size, indexing, thumbnails…), so
//    after choosing one every value can still be changed by hand.
//  - Interface presets: Windows, macOS and Linux looks on a computer (layout,
//    colors, icons, corners, window buttons and keyboard shortcuts), and
//    Google, Samsung and Apple looks on a phone or tablet.
//  - Profiles: a saved copy of every setting, unlimited in number. Choosing one
//    applies it; changes made while a profile of your own is active are saved
//    into it automatically.
//
// A preset is a plain object of "setting key -> value" (the same keys the
// settings screen uses, with advanced options as "advanced.<key>"), so presets
// and the settings screen can never disagree about what a setting is called.

import type { Profile, Settings } from "../api";
import { PRESETS as LAYOUTS } from "../util/layout";
import { getSetting, setSetting } from "./schema";

export type Patch = Record<string, unknown>;

// ---------------------------------------------------------------------------
// Performance presets
// ---------------------------------------------------------------------------

export interface PerfPreset {
  id: string;
  name: string;
  /** What it does, in a sentence or two. */
  description: string;
  /** The kind of device it suits. */
  recommendedFor: string;
  /** Roughly the memory and CPU it is meant for, used to recommend one. */
  minRamGb: number;
  minCores: number;
  /** The most of the machine it may use: percent of CPU and of memory. 0 = no cap. */
  caps: { cpu: number; memory: number };
  patch: Patch;
}

interface Level {
  id: string;
  name: string;
  threads: number;
  cacheMb: number;
  index: boolean;
  allDrives: boolean;
  listings: boolean;
  prefetch: boolean;
  thumbs: boolean;
  thumbPx: number;
  overscan: number;
  animations: boolean;
  folderSizes: boolean;
  sizeCache: boolean;
  thumbDiskMb: number;
  keepUsage: boolean;
  keepHardware: boolean;
  batterySaver: boolean;
  compactIdle: boolean;
  description: string;
  recommendedFor: string;
  minRamGb: number;
  minCores: number;
  /** Percent of all CPUs and of installed memory the app may use; 0 = no cap. */
  cpu: number;
  memory: number;
}

const LEVELS: Level[] = [
  {
    id: "barebones", name: "Barebones", threads: 1, cacheMb: 16, index: false, allDrives: false, listings: false, prefetch: false,
    thumbs: false, thumbPx: 48, overscan: 2, animations: false, folderSizes: false, sizeCache: false, thumbDiskMb: 8, keepUsage: false,
    keepHardware: false, batterySaver: true, compactIdle: true,
    description: "Does the minimum: one worker thread, almost no caching, no search index, no thumbnails, no folder sizes, no animation and nothing saved to disk. Search scans the folder you are in.",
    recommendedFor: "Very old or very small devices: a Raspberry Pi Zero, a PC with 2 GB of memory or less, a thin client, or a remote desktop session over a slow link.",
    minRamGb: 0, minCores: 1, cpu: 10, memory: 3,
},
  {
    id: "feather", name: "Feather", threads: 1, cacheMb: 32, index: false, allDrives: false, listings: true, prefetch: false,
    thumbs: false, thumbPx: 48, overscan: 3, animations: false, folderSizes: false, sizeCache: true, thumbDiskMb: 16, keepUsage: false,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Very light. Remembers a few folder listings, still no index or thumbnails, and keeps its work on one thread.",
    recommendedFor: "Old netbooks, single-board computers such as a Raspberry Pi 3, and budget tablets with 2 to 3 GB of memory.",
    minRamGb: 2, minCores: 1, cpu: 15, memory: 4,
},
  {
    id: "reduced", name: "Reduced", threads: 2, cacheMb: 64, index: false, allDrives: false, listings: true, prefetch: false,
    thumbs: true, thumbPx: 64, overscan: 4, animations: false, folderSizes: true, sizeCache: true, thumbDiskMb: 32, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Light. Small thumbnails and folder sizes come back, but there is still no background indexing, so nothing works while you are idle.",
    recommendedFor: "Older laptops and desktops with 3 to 4 GB of memory and two cores, or any machine you want kept quiet.",
    minRamGb: 3, minCores: 2, cpu: 25, memory: 6,
},
  {
    id: "acceptable", name: "Acceptable", threads: 2, cacheMb: 128, index: true, allDrives: false, listings: true, prefetch: false,
    thumbs: true, thumbPx: 64, overscan: 6, animations: false, folderSizes: true, sizeCache: true, thumbDiskMb: 64, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Search becomes instant: your home folder is indexed in the background, using two threads. No preloading yet.",
    recommendedFor: "Basic laptops and mini PCs with 4 GB of memory, such as entry-level Chromebooks running Linux or older Celeron and Atom machines.",
    minRamGb: 4, minCores: 2, cpu: 35, memory: 8,
},
  {
    id: "average", name: "Average", threads: 4, cacheMb: 256, index: true, allDrives: false, listings: true, prefetch: true,
    thumbs: true, thumbPx: 96, overscan: 8, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 128, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: false,
    description: "A sensible middle: four threads, a 256 MB cache, preloading of folders you are likely to open, and animations back on.",
    recommendedFor: "Typical office laptops and older desktops with 4 to 8 GB of memory and four cores.",
    minRamGb: 6, minCores: 4, cpu: 50, memory: 10,
},
  {
    id: "satisfactory", name: "Satisfactory", threads: 4, cacheMb: 384, index: true, allDrives: false, listings: true, prefetch: true,
    thumbs: true, thumbPx: 96, overscan: 10, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 192, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: false,
    description: "Like Average with a larger cache and a longer list of remembered folders, so going back and forth stays instant.",
    recommendedFor: "Mainstream laptops with 8 GB of memory, such as a 2018 to 2021 ultrabook.",
    minRamGb: 8, minCores: 4, cpu: 50, memory: 12,
},
  {
    id: "decent", name: "Decent", threads: 6, cacheMb: 512, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 128, overscan: 10, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 256, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: false,
    description: "Indexes every local drive, uses six threads and makes sharper thumbnails.",
    recommendedFor: "Good laptops and desktops with 8 to 16 GB of memory and six cores, and people who keep files on several drives.",
    minRamGb: 8, minCores: 6, cpu: 60, memory: 15,
},
  {
    id: "performance", name: "Performance", threads: 8, cacheMb: 1024, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 128, overscan: 12, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 512, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: false,
    description: "Built for speed on capable hardware: eight threads, a 1 GB cache, and generous preloading.",
    recommendedFor: "Modern laptops and desktops with 16 GB of memory and eight cores, such as a recent Core i7 or Ryzen 7.",
    minRamGb: 16, minCores: 8, cpu: 70, memory: 20,
},
  {
    id: "power", name: "Power", threads: 12, cacheMb: 2048, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 160, overscan: 16, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 1024, keepUsage: true,
    keepHardware: true, batterySaver: false, compactIdle: false,
    description: "Heavy use of the machine: twelve threads, a 2 GB cache, and it no longer slows down on battery.",
    recommendedFor: "Workstations and gaming desktops with 16 to 32 GB of memory and twelve or more threads.",
    minRamGb: 16, minCores: 12, cpu: 80, memory: 25,
},
  {
    id: "excelling", name: "Excelling", threads: 16, cacheMb: 3072, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 192, overscan: 20, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 2048, keepUsage: true,
    keepHardware: true, batterySaver: false, compactIdle: false,
    description: "Sixteen threads and a 3 GB cache. Folders with tens of thousands of files open without a pause.",
    recommendedFor: "High-end desktops and mobile workstations with 32 GB of memory, such as a Ryzen 9 or Core i9.",
    minRamGb: 32, minCores: 16, cpu: 85, memory: 30,
},
  {
    id: "exceptional", name: "Exceptional", threads: 24, cacheMb: 4096, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 224, overscan: 24, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 4096, keepUsage: true,
    keepHardware: true, batterySaver: false, compactIdle: false,
    description: "Twenty-four threads, a 4 GB cache and very large thumbnails. Uses memory freely to avoid waiting.",
    recommendedFor: "Workstations and servers with 48 GB of memory or more and 24 or more threads, for people who handle huge folders all day.",
    minRamGb: 48, minCores: 24, cpu: 90, memory: 40,
},
  {
    id: "godlike", name: "God-like", threads: 0, cacheMb: 0, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 256, overscan: 40, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 0, keepUsage: true,
    keepHardware: true, batterySaver: false, compactIdle: false,
    description: "Everything on and nothing held back: one thread per core, no limit on processor or memory use, the largest thumbnails and no thumbnail size limit. It will use a lot of memory if you let it.",
    recommendedFor: "Top-end workstations, render machines and servers with 64 GB of memory or more and 32 or more threads. Not for laptops on battery.",
    minRamGb: 64, minCores: 32, cpu: 0, memory: 0,
},
];

function perfPatch(l: Level): Patch {
  return {
    "performance.priority": "manual",
    "performance.maxThreads": l.threads,
    "performance.maxCacheMb": l.cacheMb,
    "performance.maxCpuPercent": l.cpu,
    "performance.maxMemoryPercent": l.memory,
    "performance.backgroundIndexing": l.index,
    "performance.indexDrives": l.allDrives,
    "performance.metadataCaching": l.listings,
    "performance.predictivePrefetch": l.prefetch,
    "performance.lazyLoading": true,
    "performance.thumbnails": l.thumbs,
    "performance.thumbnailSize": l.thumbPx,
    "performance.reduceOnBattery": l.batterySaver,
    // Machines that can spare it only halve the work while minimized; the rest drop to the minimum.
    "performance.backgroundMode": l.threads === 0 || l.threads >= 16 ? "gentle" : "aggressive",
    "advanced.perf.overscanRows": l.overscan,
    "advanced.ui.animations": l.animations,
    "advanced.files.folderSizes": l.folderSizes,
    "advanced.files.sizeCache": l.sizeCache,
    // Every level that has thumbnails has them for pictures, documents and videos. How much
    // work that is stays down through the level's thread, size and cache limits instead.
    "advanced.thumbs.images": l.thumbs,
    "advanced.thumbs.documents": l.thumbs,
    "advanced.thumbs.videos": l.thumbs,
    "advanced.cache.thumbnails": l.thumbs,
    "advanced.cache.thumbnailsMaxMb": l.thumbDiskMb,
    "advanced.cache.usageSnapshots": l.keepUsage,
    "advanced.cache.predictions": l.prefetch,
    "advanced.cache.hardware": l.keepHardware,
    "advanced.perf.compactWhenIdle": l.compactIdle,
  };
}

const presetOf = (l: Level): PerfPreset => ({
  id: l.id,
  name: l.name,
  description: l.description,
  recommendedFor: l.recommendedFor,
  minRamGb: l.minRamGb,
  minCores: l.minCores,
  caps: { cpu: l.cpu, memory: l.memory },
  patch: perfPatch(l),
});

export const PERF_PRESETS: PerfPreset[] = LEVELS.map(presetOf);

// Phones and tablets. A phone shares a few gigabytes between every open app, runs on a
// battery, and closes whichever app holds the most memory when it runs short. So these
// are far lighter than the computer presets at the same memory size: the heaviest one
// here uses less than "Average" does on a computer. Every step gives memory back when the
// app is in the background and slows down on battery; none of them may be told not to.
const MOBILE_LEVELS: Level[] = [
  {
    id: "m-essential", name: "Essential", threads: 1, cacheMb: 8, index: false, allDrives: false, listings: false, prefetch: false,
    thumbs: false, thumbPx: 48, overscan: 2, animations: false, folderSizes: false, sizeCache: false, thumbDiskMb: 0, keepUsage: false,
    keepHardware: false, batterySaver: true, compactIdle: true,
    description: "The least it can do: one worker thread, nothing kept in memory or on storage, no thumbnails, no folder sizes, no search index and no animation. Search looks through the folder you are in.",
    recommendedFor: "Phones with 2 GB of memory or less: Android Go phones, phones from before about 2018, and very old tablets.",
    minRamGb: 0, minCores: 1, cpu: 8, memory: 2,
  },
  {
    id: "m-light", name: "Light", threads: 1, cacheMb: 16, index: false, allDrives: false, listings: true, prefetch: false,
    thumbs: true, thumbPx: 48, overscan: 3, animations: false, folderSizes: false, sizeCache: false, thumbDiskMb: 16, keepUsage: false,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Still one thread. Remembers the last few folders so going back is instant, and shows small thumbnails. No folder sizes, search index or animation.",
    recommendedFor: "Phones with 3 GB of memory: entry-level phones and older tablets.",
    minRamGb: 3, minCores: 4, cpu: 10, memory: 2,
  },
  {
    id: "m-balanced", name: "Balanced", threads: 2, cacheMb: 32, index: false, allDrives: false, listings: true, prefetch: false,
    thumbs: true, thumbPx: 64, overscan: 4, animations: true, folderSizes: false, sizeCache: true, thumbDiskMb: 48, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Two threads and a 32 MB cache. Thumbnails are kept on storage so picture folders open quickly, and animations are on. Folder sizes are shown only where they were measured before.",
    recommendedFor: "Phones with 4 GB of memory: most budget phones of the last few years.",
    minRamGb: 4, minCores: 6, cpu: 15, memory: 3,
  },
  {
    id: "m-smooth", name: "Smooth", threads: 2, cacheMb: 64, index: false, allDrives: false, listings: true, prefetch: false,
    thumbs: true, thumbPx: 96, overscan: 6, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 96, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Sharper thumbnails and folder sizes measured in the background, still on two threads. No search index, so nothing runs while the app is idle.",
    recommendedFor: "Phones with 6 GB of memory: mid-range phones and most tablets.",
    minRamGb: 6, minCores: 8, cpu: 20, memory: 4,
  },
  {
    id: "m-fast", name: "Fast", threads: 3, cacheMb: 96, index: true, allDrives: false, listings: true, prefetch: true,
    thumbs: true, thumbPx: 128, overscan: 8, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 160, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "Search becomes instant: built-in storage is indexed in the background on three threads, and folders you are likely to open next are prepared early.",
    recommendedFor: "Phones with 8 GB of memory: flagship phones of the last few years and good tablets.",
    minRamGb: 8, minCores: 8, cpu: 25, memory: 5,
  },
  {
    id: "m-max", name: "Maximum", threads: 4, cacheMb: 160, index: true, allDrives: true, listings: true, prefetch: true,
    thumbs: true, thumbPx: 160, overscan: 10, animations: true, folderSizes: true, sizeCache: true, thumbDiskMb: 256, keepUsage: true,
    keepHardware: true, batterySaver: true, compactIdle: true,
    description: "The most a phone should give one app: four threads, a 160 MB cache, large thumbnails and an index of the memory card as well. It still slows down on battery and gives memory back in the background.",
    recommendedFor: "Phones and tablets with 12 GB of memory or more: current flagships, foldables and large tablets.",
    minRamGb: 12, minCores: 8, cpu: 35, memory: 6,
  },
];

export const MOBILE_PERF_PRESETS: PerfPreset[] = MOBILE_LEVELS.map(presetOf);

/** The performance presets for the kind of device: a computer, or a phone or tablet. */
export function perfPresetsFor(mobile: boolean): PerfPreset[] {
  return mobile ? MOBILE_PERF_PRESETS : PERF_PRESETS;
}

/** Which phone preset suits a device: the strongest one its memory AND cores support. */
export function recommendMobilePerf(totalRamMb: number, cpus: number): PerfPreset {
  // A phone reports noticeably less than the size on its box (an "8 GB" phone says about 7.3), so round up.
  const gb = Math.ceil(totalRamMb / 1024 - 0.05);
  let best = 0;
  MOBILE_PERF_PRESETS.forEach((p, i) => {
    if (gb >= p.minRamGb && cpus >= p.minCores) best = i;
  });
  return MOBILE_PERF_PRESETS[best];
}

/** Which performance preset suits a machine: the strongest one its memory AND cores support. */
export function recommendPerf(totalRamMb: number, cpus: number, laptop: boolean): PerfPreset {
  const gb = totalRamMb / 1024 + 0.25; // installed memory is reported a little under its label
  let best = 0;
  PERF_PRESETS.forEach((p, i) => {
    if (gb >= p.minRamGb && cpus >= p.minCores) best = i;
  });
  // Machines on battery do better one step down, and a laptop never needs the top steps.
  if (laptop) best = Math.min(Math.max(0, best - 1), PERF_PRESETS.findIndex((p) => p.id === "performance"));
  return PERF_PRESETS[best];
}

// ---------------------------------------------------------------------------
// Interface presets (Windows, macOS, Linux; Google, Samsung, Apple on phones)
// ---------------------------------------------------------------------------

export type Platform = "windows" | "macos" | "linux" | "google" | "samsung" | "apple";

export interface UiPreset {
  id: Platform;
  name: string;
  description: string;
  recommendedFor: string;
  /** Colors: the dark and light themes to use. */
  themes: { dark: string; light: string };
  patch: Patch;
  /** Values set when the style is chosen but not part of what it "is", so changing them later does not make the style read as customized. */
  start?: Patch;
  /** A phone and tablet look, modelled on a phone's own file manager. */
  mobile?: boolean;
}

const layoutOf = (id: string) => LAYOUTS.find((l) => l.id === id)!.layout;

/** Keyboard shortcuts that differ from the app's defaults. Ctrl means Cmd on a Mac. */
const MAC_KEYS: Record<string, string> = {
  open: "Ctrl+ArrowDown",
  rename: "Enter",
  delete: "Ctrl+Backspace",
  newFolder: "Ctrl+Shift+N",
  properties: "Ctrl+I",
  goUp: "Ctrl+ArrowUp",
  back: "Ctrl+[",
  forward: "Ctrl+]",
  focusAddress: "Ctrl+Shift+G",
  focusSearch: "Ctrl+F",
  toggleHidden: "Ctrl+Shift+>",
  newTab: "Ctrl+T",
  closeTab: "Ctrl+W",
  settings: "Ctrl+,",
  toggleSidebar: "Ctrl+Alt+S",
  toggleDetails: "Ctrl+Alt+I",
  home: "Ctrl+Shift+H",
  thisPc: "Ctrl+Shift+C",
};

const LINUX_KEYS: Record<string, string> = {
  open: "Enter",
  rename: "F2",
  delete: "Delete",
  newFolder: "Ctrl+Shift+N",
  properties: "Alt+Enter",
  goUp: "Alt+ArrowUp",
  back: "Alt+ArrowLeft",
  forward: "Alt+ArrowRight",
  focusAddress: "Ctrl+L",
  focusSearch: "Ctrl+F",
  toggleHidden: "Ctrl+H",
  toggleSidebar: "F9",
  home: "Alt+Home",
};

export const UI_PRESETS: UiPreset[] = [
  {
    id: "windows",
    name: "Windows",
    description: "Arranged like File Explorer in Windows 11: tabs, then the address bar, then the command bar; navigation pane on the left, details view. Windows 11 colors, rounded corners, Fluent-style icons, window buttons on the right and Windows shortcuts (F2 rename, Alt+Up, Alt+Enter).",
    recommendedFor: "Anyone used to Windows, on any system.",
    themes: { dark: "win11-dark", light: "win11-light" },
    start: { "appearance.viewLevel": 0 },
    patch: {
      "appearance.layout": layoutOf("modern"),
      "advanced.ui.chrome": "windows",
      "advanced.ui.statusBar": true,
      "appearance.density": "comfortable",
      "appearance.style.icons": "fluent",
      "appearance.style.radius": 8,
      "appearance.style.menu": "glass",
      "appearance.style.folderColor": "classic",
      "appearance.style.font": "system",
      "appearance.showDetailsPane": false,
      "advanced.ui.windowControlsSide": "right",
      "advanced.files.singleClickOpen": false,
      "advanced.files.sizeUnits": "windows",
      keys: {},
    },
  },
  {
    id: "macos",
    name: "macOS",
    description: "Arranged like Finder: one toolbar across the top with the window buttons, back and forward, the folder name, view, sort and share buttons and search; tabs appear only when you open a second one; a Favorites and Locations sidebar; the path along the bottom under the item count; icon view. Thin icons, blue folders and Mac shortcuts (Return renames, Cmd+Down opens, Cmd+[ and ] for back and forward).",
    recommendedFor: "Mac users, or anyone who prefers Finder's layout.",
    themes: { dark: "mac-dark", light: "mac-light" },
    start: { "appearance.viewLevel": 3 },
    patch: {
      "appearance.layout": {
        ...layoutOf("modern"),
        address: { ...layoutOf("modern").address, order: 0 },
        commands: { ...layoutOf("modern").commands, dock: "popout", order: 0 },
        tabs: { ...layoutOf("modern").tabs, order: 1 },
        details: { ...layoutOf("modern").details, dock: "hidden" },
      },
      "advanced.ui.chrome": "macos",
      "advanced.ui.statusBar": true,
      "appearance.density": "comfortable",
      "appearance.style.icons": "thin",
      "appearance.style.radius": 10,
      "appearance.style.menu": "glass",
      "appearance.style.folderColor": "blue",
      "appearance.style.font": "ui",
      "appearance.showDetailsPane": false,
      "advanced.ui.windowControlsSide": "left",
      "advanced.files.singleClickOpen": false,
      "advanced.files.sizeUnits": "decimal",
      keys: MAC_KEYS,
    },
  },
  {
    id: "linux",
    name: "Linux",
    description: "Arranged like GNOME Files: one header bar with back and forward, the path, search, view and menu buttons and the window buttons; tabs appear only when you open a second one; a Places sidebar; no command bar or status bar (the same actions are in the menu and the right-click menu); icon view. Adwaita-style colors, outline icons and the usual Linux shortcuts (Ctrl+L, Ctrl+H, F9).",
    recommendedFor: "Linux users, or anyone who prefers a simpler window.",
    themes: { dark: "adwaita-dark", light: "adwaita-light" },
    start: { "appearance.viewLevel": 4 },
    patch: {
      "appearance.layout": {
        ...layoutOf("modern"),
        address: { ...layoutOf("modern").address, order: 0 },
        tabs: { ...layoutOf("modern").tabs, order: 1 },
        commands: { ...layoutOf("modern").commands, dock: "popout", order: 0 },
        details: { ...layoutOf("modern").details, dock: "hidden" },
      },
      "advanced.ui.chrome": "linux",
      "advanced.ui.statusBar": false,
      "appearance.density": "comfortable",
      "appearance.style.icons": "outline",
      "appearance.style.radius": 12,
      "appearance.style.menu": "floating",
      "appearance.style.folderColor": "blue",
      "appearance.style.font": "ui",
      "appearance.showDetailsPane": false,
      "advanced.ui.windowControlsSide": "right",
      "advanced.files.singleClickOpen": false,
      "advanced.files.sizeUnits": "decimal",
      keys: LINUX_KEYS,
    },
  },
  // ---- Phones and tablets. The phone layout has no docks or window buttons, so these
  // ---- set the arrangement, colors, shapes and how items open, and nothing else.
  {
    id: "google",
    name: "Google",
    mobile: true,
    description: "Arranged like Files by Google: a rounded search bar on top, a Browse page with categories and storage, a bar along the bottom to switch pages, and a simple list with a menu button on every row. Material colors, pill-shaped buttons and large rounded sheets.",
    recommendedFor: "Pixel, Motorola, Nokia, OnePlus and most other Android phones.",
    themes: { dark: "material-dark", light: "material-light" },
    start: { "appearance.viewLevel": 0 },
    patch: {
      "advanced.ui.chrome": "google",
      "advanced.ui.statusBar": false,
      "appearance.density": "comfortable",
      "appearance.style.icons": "outline",
      "appearance.style.radius": 16,
      "appearance.style.menu": "floating",
      "appearance.style.folderColor": "mono",
      "appearance.style.font": "ui",
      "appearance.showDetailsPane": false,
      "advanced.files.singleClickOpen": true,
      "advanced.files.sizeUnits": "decimal",
      keys: {},
    },
  },
  {
    id: "samsung",
    name: "Samsung",
    mobile: true,
    description: "Arranged like My Files on a Galaxy: a large title that leaves room for your thumb, categories as round colored buttons, storage and tools in rounded cards, the path across the top of every folder, and Move, Copy, Share and Delete along the bottom while you select. One UI colors and yellow folders.",
    recommendedFor: "Samsung Galaxy phones and tablets.",
    themes: { dark: "oneui-dark", light: "oneui-light" },
    start: { "appearance.viewLevel": 0 },
    patch: {
      "advanced.ui.chrome": "samsung",
      "advanced.ui.statusBar": false,
      "appearance.density": "comfortable",
      "appearance.style.icons": "bold",
      "appearance.style.radius": 22,
      "appearance.style.menu": "floating",
      "appearance.style.folderColor": "classic",
      "appearance.style.font": "ui",
      "appearance.showDetailsPane": false,
      "advanced.files.singleClickOpen": true,
      "advanced.files.sizeUnits": "decimal",
      keys: {},
    },
  },
  {
    id: "apple",
    name: "Apple",
    mobile: true,
    description: "Arranged like Files on an iPhone: a large Browse title with a search field, Locations and Favorites in grouped lists, Recents and Browse in a bar along the bottom, blue folders in a grid, a back button that names where you came from, and a round menu button for Select, New Folder, view and sorting.",
    recommendedFor: "iPhone and iPad.",
    themes: { dark: "ios-dark", light: "ios-light" },
    start: { "appearance.viewLevel": 3 },
    patch: {
      "advanced.ui.chrome": "apple",
      "advanced.ui.statusBar": false,
      "appearance.density": "comfortable",
      "appearance.style.icons": "thin",
      "appearance.style.radius": 12,
      "appearance.style.menu": "glass",
      "appearance.style.folderColor": "blue",
      "appearance.style.font": "ui",
      "appearance.showDetailsPane": false,
      "advanced.files.singleClickOpen": true,
      "advanced.files.sizeUnits": "decimal",
      keys: {},
    },
  },
];

/** The interface presets for the kind of device. */
export function uiPresetsFor(mobile: boolean): UiPreset[] {
  return UI_PRESETS.filter((p) => !!p.mobile === mobile);
}

/**
 * The interface preset that matches this device. On Android the maker decides between the
 * Samsung and the Google look; `maker` is the manufacturer's name when it is known.
 */
export function platformFor(os: string | undefined, maker?: string): Platform {
  if (os === "ios") return "apple";
  if (os === "android") return /samsung/i.test(maker ?? "") ? "samsung" : "google";
  return os === "macos" ? "macos" : os === "linux" ? "linux" : "windows";
}

/** The phone look that goes with a system when the phone layout is shown on a computer. */
export function mobilePlatformFor(os: string | undefined, maker?: string): Platform {
  if (os === "macos" || os === "ios") return "apple";
  return /samsung/i.test(maker ?? "") ? "samsung" : "google";
}

// ---------------------------------------------------------------------------
// Applying presets and working with profiles
// ---------------------------------------------------------------------------

/** Apply a preset's patch to a settings object (mutates it). */
export function applyPatch(settings: Settings, patch: Patch): void {
  for (const [key, value] of Object.entries(patch)) {
    if (key === "performance.backgroundMode" && settings.performance.backgroundLocked) continue;
    if (key === "keys") settings.keys = { ...(value as Record<string, string>) };
    else setSetting(settings, key, structuredClone(value));
  }
}

/** Apply an interface preset, choosing the dark or light variant of its theme. */
export function applyUiPreset(settings: Settings, preset: UiPreset, systemDark: boolean): void {
  applyPatch(settings, preset.patch);
  if (preset.start) applyPatch(settings, preset.start);
  const current = settings.appearance.theme;
  const wantDark = current === "system" ? systemDark : !/light/i.test(current) && !["sepia", "rose", "solarized-light"].includes(current);
  settings.appearance.theme = wantDark ? preset.themes.dark : preset.themes.light;
  settings.appearance.accent = "#4cc2ff"; // let the theme's own accent show
}

/** JSON with object keys in a fixed order, so equal values compare equal however they were built or saved. */
function stable(v: unknown): string {
  return JSON.stringify(v, (_k, x) => (x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => a.localeCompare(b))) : x));
}

/** Which of a preset's values differ from the current settings (empty means "already applied"). */
export function differences(settings: Settings, patch: Patch): string[] {
  return Object.entries(patch)
    .filter(([key, value]) => {
      if (key === "performance.backgroundMode" && settings.performance.backgroundLocked) return false;
      if (key === "keys") return stable(sorted(settings.keys ?? {})) !== stable(sorted(value as Record<string, string>));
      return stable(getSetting(settings, key)) !== stable(value);
    })
    .map(([key]) => key);
}

const sorted = (o: Record<string, string>) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => a.localeCompare(b)));

/** The preset whose values all match the settings, if any (computer and phone presets alike). */
export function currentPerfPreset(settings: Settings): PerfPreset | undefined {
  return [...PERF_PRESETS, ...MOBILE_PERF_PRESETS].find((p) => differences(settings, p.patch).length === 0);
}

/** Everything that belongs in a profile: the settings without the profile bookkeeping. */
export function snapshotOf(settings: Settings): Record<string, unknown> {
  const copy = structuredClone(settings) as unknown as Record<string, unknown>;
  delete copy.profiles;
  delete copy.activeProfile;
  delete copy.setupDone;
  // The access token belongs to this computer, not to a profile or an export.
  if (isObject(copy.updates)) delete copy.updates.token;
  return copy;
}

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);

function merge(into: Record<string, unknown>, from: Record<string, unknown>): void {
  for (const [k, v] of Object.entries(from)) {
    if (isObject(v) && isObject(into[k])) merge(into[k] as Record<string, unknown>, v);
    else into[k] = structuredClone(v);
  }
}

/** Apply a saved profile. Options added to the app since it was saved keep their current values. */
export function applyProfile(settings: Settings, profile: Profile): void {
  const target = settings as unknown as Record<string, unknown>;
  const { advanced, keys, ...rest } = profile.snapshot as { advanced?: Record<string, unknown>; keys?: Record<string, string> } & Record<string, unknown>;
  merge(target, rest);
  if (advanced) {
    // Where the pane was folded is about this window, not about a style of working.
    const local = Object.fromEntries(Object.entries(settings.advanced).filter(([k]) => k === "sidebar.folded"));
    settings.advanced = { ...structuredClone(advanced), ...local };
  }
  if (keys) settings.keys = { ...keys };
  settings.activeProfile = profile.id;
}

export function newProfile(settings: Settings, name: string, description = ""): Profile {
  const id = `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
  return { id, name: name.trim() || "My profile", description, builtIn: false, snapshot: snapshotOf(settings) };
}

/** "Uses at most 50% of the processor and 10% of memory", or that nothing is capped. */
export function capsText(p: PerfPreset): string {
  if (!p.caps.cpu && !p.caps.memory) return "No cap: it may use as much of the processor and memory as it needs.";
  return `Uses at most ${p.caps.cpu}% of the processor and ${p.caps.memory}% of memory (you can change both in Settings > Performance).`;
}
