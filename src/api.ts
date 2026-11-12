// Typed bridge to the Rust backend. Every function here maps 1:1 to a
// command in src-tauri/src/commands.rs; the types mirror the Rust structs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface FileEntry {
  name: string;
  isDir: boolean;
  isSymlink: boolean;
  isHidden: boolean;
  size: number;
  modified: number | null;
  created: number | null;
}

export interface Listing {
  path: string;
  parent: string | null;
  entries: FileEntry[];
  cached: boolean;
}

export type Media = "ssd" | "hdd" | "network" | "removable" | "unknown";

export interface DriveName {
  name: string;
  /** The system's name for the drive when this one was chosen. */
  base: string;
  hideAddress: boolean | null;
}

export interface Drive {
  name: string;
  mount: string;
  fs: string;
  total: number;
  available: number;
  media: Media;
  removable: boolean;
  readOnly: boolean;
  online: boolean;
  /** Network shares: where the drive points, e.g. a UNC path to the share. */
  remote?: string | null;
}

export interface Details {
  name: string;
  path: string;
  parent: string | null;
  kind: "file" | "folder" | "symlink";
  size: number;
  sizeOnDisk: number | null;
  created: number | null;
  modified: number | null;
  accessed: number | null;
  readonly: boolean;
  hidden: boolean;
  extension: string | null;
  mime: string | null;
  symlinkTarget: string | null;
  unix: { mode: string; octal: string; uid: number; gid: number; inode: number; hardLinks: number } | null;
  attributes: string[];
  image: [number, number] | null;
  itemCount: number | null;
  volume: Drive | null;
}

export interface PathStat {
  path: string;
  value: number;
}

export interface FolderStats {
  files: number;
  folders: number;
  totalBytes: number;
  maxDepth: number;
  largest: PathStat[];
  byType: { extension: string; count: number; bytes: number }[];
  newest: PathStat | null;
  oldest: PathStat | null;
  emptyFolders: number;
  complete: boolean;
}

export interface Hit {
  path: string;
  name: string;
  isDir: boolean;
  size: number;
  modified: number;
}

export interface SearchResponse {
  hits: Hit[];
  source: "index" | "live";
  complete: boolean;
  elapsedMs: number;
  searchedEntries: number;
}

export interface SizeNode {
  name: string;
  path: string;
  size: number;
  isDir: boolean;
  children: SizeNode[];
  /** Biggest file types inside a folder: [index into Analysis.extNames, bytes]. */
  types: [number, number][];
}

export interface Analysis {
  tree: SizeNode;
  largest: { path: string; name: string; size: number; modified: number }[];
  totalBytes: number;
  fileCount: number;
  complete: boolean;
  extNames: string[];
  types: { ext: number; bytes: number; files: number }[];
}

export interface AnalysisResult {
  path: string;
  analysis: Analysis;
  /** Unix seconds when the numbers were measured. */
  takenAt: number;
}

export interface AnalysisProgress {
  path: string;
  entries: number;
  bytes: number;
  elapsedSecs: number;
}

export type TransferOp = "copy" | "move";
export type Conflict = "skip" | "overwrite" | "keepBoth";

export interface TransferSpec {
  sources: string[];
  destDir: string;
  op: TransferOp;
  conflict: Conflict;
}

export interface TransferError {
  path: string;
  message: string;
  needsElevation: boolean;
}

export interface Progress {
  id: number;
  op: TransferOp;
  phase: "scanning" | "running" | "paused" | "done" | "cancelled" | "failed";
  strategy: "rename" | "largeSequential" | "smallParallel" | "mixed" | null;
  destDir: string;
  filesTotal: number;
  filesDone: number;
  bytesTotal: number;
  bytesDone: number;
  skipped: number;
  bytesPerSec: number;
  filesPerSec: number;
  etaSecs: number | null;
  /** 0 = rough guess, 1 = settled. */
  etaConfidence: number;
  display: "bytes" | "files" | "mixed";
  overheadShare: number;
  current: string;
  errors: TransferError[];
  errorCount: number;
  elapsedSecs: number;
  /** 0..1, only ever increases. */
  fraction: number;
  avgBytesPerSec: number;
  avgFilesPerSec: number;
  peakBytesPerSec: number;
  peakFilesPerSec: number;
  route: Route | null;
  plan: Plan | null;
  prediction: Prediction | null;
}

export interface Route {
  src: string;
  dst: string;
  srcMedia: Media;
  dstMedia: Media;
  srcName: string;
  dstName: string;
}

export interface Plan {
  strategy: NonNullable<Progress["strategy"]>;
  smallWorkers: number;
  largeWorkers: number;
  bufferBytes: number;
  largeThreshold: number;
  tryClone: boolean;
}

export interface Prediction {
  params: { perFileSecs: number; perMbSecs: number };
  typicalSecs: number;
  samples: number;
  sameRoute: boolean;
}

export interface DeleteProgress {
  id: number;
  phase: "scanning" | "running" | "done" | "cancelled" | "failed";
  permanent: boolean;
  selected: number;
  itemsTotal: number;
  itemsDone: number;
  current: string;
  errors: TransferError[];
  errorCount: number;
  elapsedSecs: number;
  /** Name of the first selected item. */
  firstName: string;
}

/** Work that can be run through the operating system's administrator prompt. */
export type Job =
  | { kind: "transfer"; spec: TransferSpec }
  | { kind: "delete"; paths: string[]; permanent: boolean }
  | { kind: "rename"; path: string; newName: string }
  | { kind: "create"; dir: string; name: string; folder: boolean };

export interface AccessCheck {
  needsElevation: boolean;
  becomesPermanent: boolean;
}

export interface JobFinished {
  id: number;
  declined: boolean;
  ok: boolean;
  error: string | null;
  path: string | null;
}

export interface CacheEntry {
  id: string;
  label: string;
  description: string;
  diskBytes: number;
  memoryBytes: number;
  items: number;
  rebuildable: boolean;
}

export interface HistoryInfo {
  records: number;
  bytes: number;
  routes: number;
}

export interface Style {
  font: string;
  fontSize: number;
  menu: "glass" | "flat" | "outlined" | "floating" | string;
  icons: "outline" | "bold" | "thin" | string;
  folderColor: "classic" | "blue" | "mono" | "accent" | string;
  background: "plain" | "gradient" | "aurora" | "dusk" | "mist" | "image" | string;
  backgroundImage: string;
  radius: number;
  enabledSheets: string[];
}

export interface ShellItem {
  id: number;
  label: string;
  iconUrl: string | null;
  disabled: boolean;
  checked: boolean;
  children: ShellItem[];
}

export interface Asset {
  name: string;
  bytes: number;
}

export type AssetKind = "font" | "css" | "image";

export interface Settings {
  general: {
    showHidden: boolean;
    showExtensions: boolean;
    openSearchResultInNewTab: boolean;
    restoreSession: boolean;
    confirmDelete: boolean;
    deleteToTrash: boolean;
    startPage: "home" | "thisPc";
    foldersFirst: boolean;
    nativeProperties: boolean;
    shellExtras: boolean;
  };
  appearance: {
    theme: string;
    accent: string;
    density: "compact" | "comfortable";
    customCss: string;
    driveChart: "bar" | "donut";
    driveMetric: "used" | "free";
    showDetailsPane: boolean;
    /** 0 details, 1 list, 2 small, 3 medium, 4 large, 5 extra large icons. */
    viewLevel: number;
    sidebarWidth: number;
    sidebarCollapsed: boolean;
    showFrequent: boolean;
    showPinned: boolean;
    showDrives: boolean;
    showDriveSpace: boolean;
    hideNetworkAddress: boolean;
    driveNames: Record<string, DriveName>;
    layout: unknown;
    style: Style;
  };
  performance: {
    priority: "dynamic" | "manual";
    maxThreads: number;
    maxCacheMb: number;
    backgroundIndexing: boolean;
    indexAllDrives: boolean;
    lazyLoading: boolean;
    metadataCaching: boolean;
    thumbnails: boolean;
    thumbnailSize: number;
    predictivePrefetch: boolean;
    reduceOnBattery: boolean;
  };
  transfer: {
    conflict: "ask" | Conflict;
    preserveTimestamps: boolean;
    largeFileThresholdMb: number;
    maxParallel: number;
    rememberDeviceSpeeds: boolean;
    historyLimit: number;
  };
  search: { limit: number; includeHidden: boolean; scopeToCurrentFolder: boolean };
  contextMenu: { hidden: string[] };
  terminal: { windowsShell: string; linuxTerminal: string; macTerminal: string };
  /** Only options that differ from their defaults (see settings/advanced.ts). */
  advanced: Record<string, unknown>;
  /** Shortcuts that differ from the defaults, by command id. */
  keys: Record<string, string>;
  customActions: CustomAction[];
}

export interface CustomAction {
  id: string;
  label: string;
  program: string;
  args: string;
  appliesTo: "any" | "file" | "folder";
  extensions: string;
  allAtOnce: boolean;
}

export interface Tab {
  id: string;
  location: string;
  groupId: string | null;
  history: string[];
  historyIndex: number;
}

export interface TabGroup {
  id: string;
  name: string;
  color: string;
  collapsed: boolean;
}

export interface Pin {
  path: string;
  label: string;
  builtin: boolean;
  isFile: boolean;
}

export interface Recent {
  path: string;
  isDir: boolean;
  at: number;
}

export interface Session {
  tabs: Tab[];
  groups: TabGroup[];
  activeTab: string | null;
  pins: Pin[] | null;
  recent: Recent[];
  drives: Drive[];
}

export interface Budget {
  tier: "full" | "balanced" | "saver" | "manual";
  ioThreads: number;
  indexThreads: number;
  thumbThreads: number;
  cacheMb: number;
  indexMaxEntries: number;
  listingCacheDirs: number;
  backgroundIndexing: boolean;
  prefetch: boolean;
  thumbnails: boolean;
}

export interface IndexStatus {
  state: "idle" | "loading" | "scanning" | "ready";
  entries: number;
  memoryBytes: number;
  builtAt: number;
  truncated: boolean;
  roots: string[];
}

export interface AppInfo {
  os: "windows" | "macos" | "linux" | string;
  version: string;
  separator: string;
  home: string | null;
  budget: Budget;
  system: {
    cpus: number;
    totalRamMb: number;
    availableRamMb: number;
    onBattery: boolean;
    batteryPercent: number | null;
  };
  index: IndexStatus;
  thumbnailTypes: string[];
}

export type TerminalMode = "here" | "home" | "driveRoot" | "admin";

export const api = {
  listDir: (path: string) => invoke<Listing>("list_dir", { path }),
  prefetch: (paths: string[]) => invoke<void>("prefetch", { paths }),
  recordVisit: (from: string | null, to: string) => invoke<string[]>("record_visit", { from, to }),
  getDetails: (path: string) => invoke<Details>("get_details", { path }),
  folderStats: (path: string) => invoke<FolderStats>("folder_stats", { path }),
  hashFile: (path: string) => invoke<string>("hash_file", { path }),
  thumbnail: (path: string, size: number) => invoke<ArrayBuffer>("thumbnail", { path, size }),
  getDrives: () => invoke<Drive[]>("get_drives"),
  knownFolders: () => invoke<{ label: string; path: string }[]>("known_folders"),
  indexStatus: () => invoke<IndexStatus>("index_status"),
  reindex: () => invoke<void>("reindex"),
  search: (query: string, scope: string | null) => invoke<SearchResponse>("search", { query, scope }),
  analysisSnapshot: (path: string) => invoke<AnalysisResult | null>("analysis_snapshot", { path }),
  analyzeStart: (path: string) => invoke<void>("analyze_start", { path }),
  analyzeCancel: (path: string) => invoke<void>("analyze_cancel", { path }),
  frequentFolders: (limit: number) => invoke<string[]>("frequent_folders", { limit }),
  checkConflicts: (sources: string[], destDir: string) =>
    invoke<string[]>("check_conflicts", { sources, destDir }),
  transferStart: (spec: TransferSpec) => invoke<number>("transfer_start", { spec }),
  transferControl: (id: number, action: "pause" | "resume" | "cancel") =>
    invoke<void>("transfer_control", { id, action }),
  checkAccess: (job: Job) => invoke<AccessCheck>("check_access", { job }),
  elevatedStart: (job: Job) => invoke<number>("elevated_start", { job }),
  deleteStart: (paths: string[], permanent: boolean) => invoke<number>("delete_start", { paths, permanent }),
  deleteControl: (id: number, action: "cancel") => invoke<void>("delete_control", { id, action }),
  showProperties: (paths: string[]) => invoke<boolean>("show_properties", { paths }),
  historyInfo: () => invoke<HistoryInfo>("history_info"),
  networkMap: (remote: string, letter: string | null) => invoke<string | null>("network_map", { remote, letter }),
  networkDisconnect: (mount: string) => invoke<void>("network_disconnect", { mount }),
  renameItem: (path: string, newName: string) => invoke<string>("rename_item", { path, newName }),
  createItem: (dir: string, name: string, folder: boolean) =>
    invoke<string>("create_item", { dir, name, folder }),
  openItem: (path: string) => invoke<void>("open_item", { path }),
  openTerminal: (path: string, mode: TerminalMode) => invoke<void>("open_terminal", { path, mode }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  getSession: () => invoke<Session>("get_session"),
  saveSession: (session: Session) => invoke<void>("save_session", { session }),
  appInfo: () => invoke<AppInfo>("app_info"),
  shellMenu: (paths: string[], background: boolean) => invoke<ShellItem[]>("shell_menu", { paths, background }),
  shellInvoke: (id: number) => invoke<void>("shell_invoke", { id }),
  createShortcut: (path: string) => invoke<string>("create_shortcut", { path }),
  importAsset: (kind: AssetKind, path: string) => invoke<string>("import_asset", { kind, path }),
  listAssets: (kind: AssetKind) => invoke<Asset[]>("list_assets", { kind }),
  readAsset: (kind: AssetKind, name: string) => invoke<ArrayBuffer>("read_asset", { kind, name }),
  deleteAsset: (kind: AssetKind, name: string) => invoke<void>("delete_asset", { kind, name }),
  runCustomAction: (id: string, paths: string[]) => invoke<void>("run_custom_action", { id, paths }),
  configFolder: () => invoke<string | null>("config_folder"),
  importSettings: (json: string) => invoke<Settings>("import_settings", { json }),
  resolvePath: (input: string) => invoke<{ path: string; isDir: boolean }>("resolve_path", { input }),
  systemReport: () => invoke<FullReport>("system_report"),
  openTransfersWindow: () => invoke<void>("open_transfers_window"),
  openWindow: (location: string, x?: number, y?: number, index?: number) => invoke<void>("open_window", { location, x, y, index }),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
  reattach: (kind: "settings" | "transfers" | "tabs", payload?: unknown) => invoke<void>("reattach", { kind, payload }),
  folderSizesStart: (dir: string, names: string[], refresh: boolean) => invoke<void>("folder_sizes_start", { dir, names, refresh }),
  cacheUsage: () => invoke<CacheEntry[]>("cache_usage"),
  clearCache: (id: string) => invoke<void>("clear_cache", { id }),
};

export interface FolderSize {
  dir: string;
  name: string;
  bytes: number;
  files: number;
  /** False when the scan hit its limit: the size is a minimum. */
  complete: boolean;
}

export interface BackendEvents {
  "transfer-progress": Progress;
  "delete-progress": DeleteProgress;
  "job-finished": JobFinished;
  "analysis-progress": AnalysisProgress;
  "analysis-done": AnalysisResult;
  "analysis-partial": AnalysisResult;
  "analysis-failed": [string, string];
  "dir-updated": Listing;
  "folder-size": FolderSize;
  "fs-changed": string[];
  "index-status": IndexStatus;
  "budget-changed": Budget;
  detached: { kind: "settings" | "transfers" | string; open: boolean };
  reattach: { kind: "settings" | "transfers" | "tabs" | string; payload?: { locations?: string[]; index?: number } | null };
  "settings-changed": Settings;
}

export function on<K extends keyof BackendEvents>(
  event: K,
  handler: (payload: BackendEvents[K]) => void,
): Promise<UnlistenFn> {
  return listen<BackendEvents[K]>(event, (e) => handler(e.payload));
}

// ---- About page ------------------------------------------------------------

export interface Lifespan {
  summary: string;
  yearsLeft: number | null;
  basis: "projection" | "rule of thumb" | string;
}
export interface PhysicalDisk {
  name: string;
  model: string;
  kind: string;
  bus: string | null;
  size: number;
  health: string | null;
  temperatureC: number | null;
  powerOnHours: number | null;
  wearPercent: number | null;
  bytesWritten: number | null;
  reallocatedSectors: number | null;
  mediaErrors: number | null;
  lifespan: Lifespan | null;
  source: string;
}
export interface BatteryInfo {
  percent: number;
  state: string;
  healthPercent: number | null;
  cycles: number | null;
  vendor: string | null;
  model: string | null;
  technology: string | null;
  designWh: number | null;
  fullWh: number | null;
  lifespan: Lifespan | null;
}
export interface SystemReport {
  os: { name: string; version: string; kernel: string; arch: string; host: string; uptimeSecs: number; desktop: string | null };
  machine: { manufacturer: string | null; model: string | null; board: string | null; firmware: string | null };
  cpu: { brand: string; vendor: string; physicalCores: number | null; logical: number; mhz: number };
  memory: { total: number; available: number; swapTotal: number; swapUsed: number; modules: string[] };
  gpus: string[];
  battery: BatteryInfo | null;
  disks: PhysicalDisk[];
  software: { processCount: number; biggest: { name: string; memory: number }[]; ownMemory: number };
  notes: string[];
}
export interface FullReport {
  app: {
    version: string;
    license: string;
    profile: string;
    arch: string;
    os: string;
    tauri: string;
    webview: string | null;
    startedSecsAgo: number;
    settingsDir: string;
    cacheDir: string;
  };
  system: SystemReport;
}
