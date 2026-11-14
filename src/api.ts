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

/** What forgetting one folder or drive removed. */
export interface ForgetReport {
  listings: number;
  sizes: number;
  thumbnails: number;
  snapshots: number;
  predictions: number;
  indexEntries: number;
  bytes: number;
}

export type DeviceKind = "phone" | "camera" | "printer" | "audio" | "input" | "bluetooth" | "other";

/** A phone, camera or other connected device that is not a drive. */
export interface Device {
  id: string;
  name: string;
  maker: string;
  kind: DeviceKind;
  /** A folder the app can browse itself. */
  path: string | null;
  /** Can be mounted on request, after which `path` appears. */
  connectable: boolean;
  /** What the system's own tool opens it with when the app cannot browse it. */
  open: string | null;
  note: string;
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
  /** What a photo, song or video says about itself, in the order worth showing. */
  media: { label: string; value: string }[];
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

export interface OpenApp {
  id: string;
  name: string;
  iconUrl: string | null;
}

export interface OpenChoices {
  extension: string;
  default: OpenApp | null;
  suggested: OpenApp[];
  others: OpenApp[];
}

export interface Hit {
  /** Relevance: exact name > prefix > word start > anywhere. */
  score: number;
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
  /** True while a live walk is still streaming results. */
  searching: boolean;
}

export interface ShareInfo {
  id: string;
  name: string;
  path: string;
  size: number;
  url: string;
  expiresAt: number;
  maxDownloads: number;
  downloads: number;
  lan: boolean;
  internet: boolean;
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
  /** How busy the drive is; absent when the live graph is off. */
  activity?: { readBps: number; writeBps: number } | null;
}

export type TransferOp = "copy" | "move";
export type Conflict = "ask" | "skip" | "overwrite" | "keepBoth";

/** A file that exists on both sides, waiting for an answer while the transfer runs. */
export interface ConflictInfo {
  name: string;
  src: string;
  dst: string;
  srcSize: number;
  dstSize: number;
  /** Seconds since 1970. */
  srcModified: number | null;
  dstModified: number | null;
  number: number;
}

export interface TransferSpec {
  sources: string[];
  destDir: string;
  op: TransferOp;
  conflict: Conflict;
  /** Keep and skip a file already at the destination with the same size, time and content. */
  skipIdentical?: boolean;
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
  method?: "native" | "robocopy" | "rsync" | "rclone" | "smb" | "nfs" | null;
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
  /** Set while the transfer waits for an answer about a file that already exists. */
  conflict?: ConflictInfo | null;
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

/** What kind of connection carries the computer's traffic. */
export type NetworkLink = "wired" | "wireless" | "unknown";

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
  /** "delete", or "compress" / "extract" for an archive job shown on the same card. */
  action: "delete" | "compress" | "extract" | "upload" | "download" | "open" | "unpack" | "pack" | "remote-delete";
  /** For a finished archive or server job: what was made (for "open": the downloaded file). */
  result: string;
  /** Server jobs: bytes moved so far and expected in total. */
  bytesDone: number;
  bytesTotal: number;
  /** Files left alone because they were already there (not failures). */
  skipped?: number;
  /** A file that exists on both sides, waiting for an answer. */
  conflict?: ConflictInfo | null;
}

export type ServerProtocol = "ftp" | "sftp" | "dav";

/** Everything needed to log in to a server. The password is never saved. */
export interface Server {
  protocol: ServerProtocol;
  host: string;
  port: number;
  user: string;
  password: string;
  keyFile?: string | null;
  /** WebDAV: use HTTPS. */
  tls: boolean;
  /** SFTP: the host key fingerprint the person agreed to trust. */
  trust?: string | null;
}

/** A server address kept in the list of servers (no password). */
export interface SavedServer {
  name: string;
  protocol: ServerProtocol;
  host: string;
  port: number;
  user: string;
  tls: boolean;
  keyFile?: string | null;
  path: string;
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
    liveRefresh: boolean;
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
    maxCpuPercent: number;
    maxMemoryPercent: number;
    backgroundIndexing: boolean;
    indexDrives: boolean;
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
    method: "auto" | "native" | "robocopy" | "rsync" | "rclone" | "smb" | "nfs";
    tryOtherMethods: boolean;
    progressIntervalMs: number;
  };
  search: { limit: number; includeHidden: boolean; scopeToCurrentFolder: boolean };
  contextMenu: { hidden: string[] };
  terminal: {
    windowsShell: string;
    linuxTerminal: string;
    macTerminal: string;
    startupCommand: string;
    customCommand: string;
    windowsProfile: string;
    shellProgram: string;
    newTab: boolean;
    useSelectedFolder: boolean;
    defaultFolder: string;
  };
  /** Only options that differ from their defaults (see settings/advanced.ts). */
  advanced: Record<string, unknown>;
  /** Shortcuts that differ from the defaults, by command id. */
  keys: Record<string, string>;
  customActions: CustomAction[];
  /** Saved sets of settings (see settings/presets.ts). */
  profiles: Profile[];
  activeProfile: string;
  /** False until the first-run setup has been completed or skipped. */
  setupDone: boolean;
}

export interface Profile {
  id: string;
  name: string;
  description: string;
  builtIn: boolean;
  /** A Settings object without the profile fields. */
  snapshot: Record<string, unknown>;
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
  searches: string[];
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
  drivesReady: number;
  drivesTotal: number;
}

/** Why Smart Clear lists something, from the plainly disposable to what only the person can judge. */
export type ClearKind = "temp" | "cache" | "thumbnails" | "trash" | "partial" | "logs" | "installers" | "buildLeftovers" | "duplicates" | "oldDownloads" | "largeOld";

export interface ClearItem {
  path: string;
  /** Deleted together with `path`. */
  also: string[];
  name: string;
  size: number;
  /** Last changed or opened, seconds since 1970 (0 when unknown). */
  used: number;
  kind: ClearKind;
  isDir: boolean;
  /** Copies of one file share a number. */
  group: number | null;
  note: string;
}

export interface ClearReport {
  items: ClearItem[];
  totals: { kind: ClearKind; count: number; bytes: number; listed: number }[];
  scanned: number;
  cancelled: boolean;
  lookedIn: string[];
}

export interface ClearOptions {
  oldDays: number;
  largeMb: number;
  duplicates: boolean;
}

export interface ClearProgress {
  scanned: number;
  found: number;
  bytes: number;
  step: string;
}

/** A folder another app (or the command line) asked the app to show, and what to highlight in it. */
export interface Launch {
  folder: string;
  select: string[];
}

/** Where the app stands as the system's file manager. */
export interface DefaultFmStatus {
  supported: boolean;
  active: boolean;
  /** What handled folders before, when known. */
  previous: string | null;
  detail: string;
  /** What still goes to the system's own file manager. */
  limits: string[];
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
  remoteConnect: (server: Server, path?: string) => invoke<string>("remote_connect", { server, path: path ?? null }),
  remoteDisconnect: (location: string) => invoke<void>("remote_disconnect", { location }),
  remoteIsConnected: (location: string) => invoke<boolean>("remote_is_connected", { location }),
  remoteMkdir: (parent: string, name: string) => invoke<string>("remote_mkdir", { parent, name }),
  remoteRename: (from: string, newName: string) => invoke<string>("remote_rename", { from, newName }),
  /** Copy items out of an archive into a folder, or (open) fetch one file to a temporary place and open it. */
  archiveCopyStart: (sources: string[], dest: string, open = false, remove = false) => invoke<number>("archive_copy_start", { sources, dest, open, remove }),
  /** Put things into an archive or move them inside it. `op` "move" takes the originals away afterwards. */
  archivePasteStart: (sources: string[], dest: string, op: "copy" | "move") => invoke<number>("archive_paste_start", { sources, dest, op }),
  archiveDelete: (locs: string[]) => invoke<void>("archive_delete", { locs }),
  archiveRename: (loc: string, newName: string) => invoke<string>("archive_rename", { loc, newName }),
  archiveCreate: (dir: string, name: string, folder: boolean) => invoke<string>("archive_create", { dir, name, folder }),
  /** Unpack items to a staging folder so real files can go on the system clipboard. */
  archiveStage: (sources: string[]) => invoke<string[]>("archive_stage", { sources }),
  remoteCopyStart: (sources: string[], dest: string, mv: boolean, open = false) => invoke<number>("remote_copy_start", { sources, dest, mv, open }),
  remoteRemoveStart: (paths: string[]) => invoke<number>("remote_remove_start", { paths }),
  serversList: () => invoke<SavedServer[]>("servers_list"),
  serversSave: (entry: SavedServer) => invoke<SavedServer[]>("servers_save", { entry }),
  serversRemove: (name: string) => invoke<SavedServer[]>("servers_remove", { name }),
  serversForgetHost: (host: string, port: number) => invoke<void>("servers_forget_host", { host, port }),
  prefetch: (paths: string[]) => invoke<void>("prefetch", { paths }),
  recordVisit: (from: string | null, to: string) => invoke<string[]>("record_visit", { from, to }),
  getDetails: (path: string) => invoke<Details>("get_details", { path }),
  folderStats: (path: string) => invoke<FolderStats>("folder_stats", { path }),
  hashFile: (path: string) => invoke<string>("hash_file", { path }),
  thumbnail: (path: string, size: number) => invoke<ArrayBuffer>("thumbnail", { path, size }),
  /** A whole file, for the app's own viewer (phones that cannot hand a file to another app). */
  readForView: (path: string) => invoke<ArrayBuffer>("read_for_view", { path }),
  getDrives: () => invoke<Drive[]>("get_drives"),
  getDevices: () => invoke<Device[]>("get_devices"),
  networkLink: () => invoke<NetworkLink>("network_link"),
  connectDevice: (id: string) => invoke<string | null>("connect_device", { id }),
  openDeviceInSystem: (open: string) => invoke<void>("open_device_in_system", { open }),
  knownFolders: () => invoke<{ label: string; path: string }[]>("known_folders"),
  indexStatus: () => invoke<IndexStatus>("index_status"),
  reindex: () => invoke<void>("reindex"),
  search: (query: string, scope: string | null, id: number) => invoke<SearchResponse>("search", { query, scope, id }),
  analysisSnapshot: (path: string) => invoke<AnalysisResult | null>("analysis_snapshot", { path }),
  analyzeStart: (path: string) => invoke<void>("analyze_start", { path }),
  analyzeCancel: (path: string) => invoke<void>("analyze_cancel", { path }),
  frequentFolders: (limit: number) => invoke<string[]>("frequent_folders", { limit }),
  trashPaths: (paths: string[]) => invoke<void>("trash_paths", { paths }),
  restoreFromTrash: (paths: string[]) => invoke<number>("restore_from_trash", { paths }),
  archiveStart: (action: "compress" | "extract", paths: string[], here = false) => invoke<number>("archive_start", { action, paths, here }),
  watchDirs: (dirs: string[]) => invoke<void>("watch_dirs", { dirs }),
  checkConflicts: (sources: string[], destDir: string) =>
    invoke<string[]>("check_conflicts", { sources, destDir }),
  transferStart: (spec: TransferSpec) => invoke<number>("transfer_start", { spec }),
  /** Put files on the system clipboard, so other programs can paste them. */
  clipboardSetFiles: (paths: string[]) => invoke<void>("clipboard_set_files", { paths }),
  clipboardGetFiles: () => invoke<string[]>("clipboard_get_files"),
  /** What another program put on the clipboard: paths, whether it was a Cut, and files that exist only inside that program. */
  clipboardGet: () => invoke<{ paths: string[]; cut: boolean; virtualFiles: boolean }>("clipboard_get"),
  clipboardClear: () => invoke<void>("clipboard_clear"),
  clipboardStageVirtual: () => invoke<string[]>("clipboard_stage_virtual"),
  /** Start the system drag with these files, to drop them into another program. */
  dragOut: (paths: string[]) => invoke<void>("drag_out", { paths }),
  transferResolve: (id: number, answer: "skip" | "replace" | "keepBoth", all: boolean) =>
    invoke<void>("transfer_resolve", { id, answer, all }),
  transferForceStop: (id: number) => invoke<void>("transfer_force_stop", { id }),
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
  driveEject: (mount: string) => invoke<void>("drive_eject", { mount }),
  renameItem: (path: string, newName: string) => invoke<string>("rename_item", { path, newName }),
  createItem: (dir: string, name: string, folder: boolean) =>
    invoke<string>("create_item", { dir, name, folder }),
  /** Resolves with the name of the program used when it was picked as the likely one (the system had none set). */
  openItem: (path: string) => invoke<string | null>("open_item", { path }),
  openChoices: (path: string) => invoke<OpenChoices>("open_choices", { path }),
  openWithApp: (path: string, app: string, always: boolean) => invoke<void>("open_with_app", { path, app, always }),
  openSystemChooser: (path: string) => invoke<void>("open_system_chooser", { path }),
  openTerminal: (path: string, mode: TerminalMode) => invoke<void>("open_terminal", { path, mode }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  getSession: () => invoke<Session>("get_session"),
  saveSession: (session: Session) => invoke<void>("save_session", { session }),
  appInfo: () => invoke<AppInfo>("app_info"),
  smartClearScan: (drive: string | null, options: ClearOptions) => invoke<ClearReport>("smart_clear_scan", { drive, options }),
  smartClearCancel: () => invoke<void>("smart_clear_cancel"),
  takeLaunch: () => invoke<Launch[]>("take_launch"),
  defaultFmStatus: () => invoke<DefaultFmStatus>("default_fm_status"),
  defaultFmSet: (on: boolean) => invoke<DefaultFmStatus>("default_fm_set", { on }),
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
  systemReport: (refresh = false) => invoke<FullReport>("system_report", { refresh }),
  diskSmartAdmin: (device: string) => invoke<FullReport>("disk_smart_admin", { device }),
  resourceUsage: () => invoke<ResourceUsage>("resource_usage"),
  compactMemory: () => invoke<[number, number]>("compact_memory"),
  locksList: () => invoke<LocksState>("locks_list"),
  lockAdd: (path: string, passcode: string, mainPassword?: string) => invoke<LocksState>("lock_add", { path, passcode, mainPassword }),
  lockUnlock: (path: string, passcode: string) => invoke<boolean>("lock_unlock", { path, passcode }),
  lockRemove: (id: string, secret: string) => invoke<LocksState>("lock_remove", { id, secret }),
  lockSetMain: (old: string, next: string) => invoke<LocksState>("lock_set_main", { old, new: next }),
  locksRelock: () => invoke<LocksState>("locks_relock"),
  openTransfersWindow: () => invoke<void>("open_transfers_window"),
  openWindow: (location: string, x?: number, y?: number) => invoke<void>("open_window", { location, x, y }),
  /** Drop a tab on another window's tab bar. True when one took it. */
  shareStart: (path: string, minutes: number, maxDownloads: number, lan: boolean, internet = false) => invoke<ShareInfo>("share_start", { path, minutes, maxDownloads, lan, internet }),
  shareInternetAvailable: () => invoke<boolean>("share_internet_available"),
  shareStop: (id: string) => invoke<void>("share_stop", { id }),
  shareList: () => invoke<ShareInfo[]>("share_list"),
  tabHover: (x: number, y: number, width: number, active: boolean) => invoke<void>("tab_hover", { x, y, width, active }),
  dropTab: (location: string, x: number, y: number, closeSource: boolean) => invoke<boolean>("drop_tab", { location, x, y, closeSource }),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
  reattach: (kind: "settings" | "transfers", payload?: unknown) => invoke<void>("reattach", { kind, payload }),
  folderSizesStart: (dir: string, names: string[], refresh: boolean, policy: SizePolicy) => invoke<void>("folder_sizes_start", { dir, names, refresh, policy }),
  cacheUsage: () => invoke<CacheEntry[]>("cache_usage"),
  clearCache: (id: string) => invoke<void>("clear_cache", { id }),
  clearCacheFor: (path: string) => invoke<ForgetReport>("clear_cache_for", { path }),
};

/** How remembered folder sizes are used. */
export interface SizePolicy {
  remember: boolean;
  freshSecs: number;
  maxEntries: number;
}

export interface FolderSize {
  /** A remembered size; a fresh measurement may follow. */
  cached?: boolean;
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
  /** A drive was connected or removed, or the free space moved. */
  "drives-changed": { drives: Drive[]; devices: Device[]; added: string[]; removed: string[] };
  "search-hits": { id: number; hits: Hit[]; scanned: number; done: boolean };
  detached: { kind: "settings" | "transfers" | string; open: boolean };
  reattach: { kind: "settings" | "transfers" | string; payload?: unknown };
  "tab-drop": { location: string; x: number; y: number };
  /** Another window is dragging a tab over this one (x is null once it leaves). */
  "tab-hover": { x: number | null; w: number };
  "settings-changed": Settings;
  "smart-clear-progress": ClearProgress;
  /** Another app asked for a folder; collect it with `takeLaunch`. */
  launch: null;
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
export interface DiskFact {
  label: string;
  value: string;
}
export interface SmartAttr {
  id: number;
  name: string;
  value: number | null;
  worst: number | null;
  threshold: number | null;
  raw: string;
  failing: boolean;
}
export interface PhysicalDisk {
  device: string;
  facts: DiskFact[];
  attributes: SmartAttr[];
  needsAdmin: boolean;
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
  /** Unix seconds when the hardware was read, if this is a saved copy. */
  cachedAt: number | null;
}

export interface ResourceUsage {
  cpuPercent: number;
  memoryBytes: number;
  virtualBytes: number;
  diskReadTotal: number;
  diskWriteTotal: number;
  threads: number;
  cpus: number;
  systemTotalBytes: number;
  systemAvailableBytes: number;
  storedDiskBytes: number;
  cachedMemoryBytes: number;
}

export interface LockInfo {
  id: string;
  path: string;
  open: boolean;
}
export interface LocksState {
  hasMain: boolean;
  locks: LockInfo[];
}
