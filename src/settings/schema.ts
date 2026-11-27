// Declarative description of every setting. The settings screen, its search
// box and its sections are all generated from this list, so adding a setting
// means adding one entry here (and the field in crates/fast-core/src/settings.rs).

import type { Settings } from "../api";
import { ADVANCED, adv } from "./advanced";

export type Control =
  /** `invert`: the switch shows the opposite of the stored value. */
  | { type: "toggle"; invert?: boolean }
  | { type: "select"; options: { value: string; label: string }[] }
  | { type: "number"; min: number; max: number; step?: number; unit?: string }
  | { type: "text"; placeholder?: string; secret?: boolean }
  | { type: "color" }
  | { type: "css" }
  | { type: "menuItems" }
  | { type: "themes" }
  | { type: "font" }
  | { type: "imports" }
  | { type: "layout" }
  | { type: "storage" }
  | { type: "activityLog" }
  | { type: "perfPresets" }
  | { type: "uiPresets" }
  | { type: "profiles" }
  | { type: "resources" }
  | { type: "locks" }
  | { type: "sshLogins" }
  | { type: "macAccess" }
  | { type: "deleteGuard" }
  | { type: "defaultFm" }
  | { type: "updates" }
  | { type: "icons" }
  | { type: "fontAreas" }
  | { type: "fontFamily"; fixedWidth?: boolean }
  | { type: "methods" };

export interface SettingDef {
  /** Dotted path into the Settings object, e.g. "general.showHidden". */
  key: string;
  section: string;
  label: string;
  description?: string;
  /** A caution shown beside the setting in a warning color. */
  warning?: string;
  control: Control;
  /** Only shown on these operating systems. */
  os?: string[];
  /** Extra words the settings search should match. */
  keywords?: string;
}

export const SECTIONS = [
  "General",
  "macOS permissions",
  "Folder views",
  "Presets and profiles",
  "Appearance",
  "Themes",
  "Layout",
  "Navigation pane",
  "Thumbnails",
  "Performance",
  "Storage",
  "Logs",
  "Privacy and locks",
  "File transfers",
  "Search",
  "Notifications",
  "Updates",
  "Right-click menu",
  "Terminal",
  "Advanced",
  "Documentation",
  "About",
] as const;

const toggle: Control = { type: "toggle" };

export const SETTINGS: SettingDef[] = [
  // ---- General ----
  { key: "general.startPage", section: "General", label: "Open new tabs to", control: { type: "select", options: [{ value: "home", label: "Home" }, { value: "thisPc", label: "This PC (drives)" }] } },
  { key: "general.restoreSession", section: "General", label: "Reopen tabs from last time", description: "Tabs and tab groups are saved as you work and restored at launch.", control: toggle, keywords: "session remember startup" },
  { key: "general.defaultFileManager", section: "General", label: "Default file manager", description: "Folders opened from other apps open here instead of in the system's file manager. Can be turned off again at any time.", control: { type: "defaultFm" }, keywords: "replace explorer finder nautilus dolphin show in folder reveal downloads open folder system default handler" },
  { key: "general.showHidden", section: "General", label: "Show hidden files and folders", control: toggle, keywords: "dotfiles" },
  { key: "general.showExtensions", section: "General", label: "Show file name extensions", control: toggle },
  { key: "general.liveRefresh", section: "General", label: "Refresh folders when they change", description: "A folder that is open updates by itself when another program adds, renames or deletes files in it.", control: toggle, keywords: "watch auto update live" },
  { key: "general.foldersFirst", section: "Folder views", label: "List folders before files", control: toggle, keywords: "sort" },
  {
    key: "general.deleteToTrash",
    section: "General",
    label: "Always delete permanently (skip the Recycle Bin / Trash)",
    description: "Off: Delete moves items to the Recycle Bin / Trash, where they can be looked at and put back. On: every delete removes items for good, without going through it. Shift+Delete always deletes permanently. With \"Ask before deleting\" turned off too, nothing asks.",
    warning: "Items deleted this way cannot be recovered.",
    control: { type: "toggle", invert: true },
    keywords: "recycle bin trash skip automatic permanent permanently delete for good shift",
  },
  { key: "general.confirmDelete", section: "General", label: "Ask before deleting", control: toggle, keywords: "confirm" },
  { key: "general.nativeProperties", section: "General", label: "Use the system's Properties window", description: "Opens Windows' own Properties (or the equivalent). Turn off to always use the app's own window, which also works where the system has none.", control: toggle, keywords: "properties dialog native" },
  { key: "general.shellExtras", section: "General", label: "Add entries from other programs to the right-click menu", description: "Edit with…, Open with, archivers and similar. They load a moment after the menu opens, so the menu itself is never slowed down. Turn off to skip them.", control: toggle, keywords: "context menu shell extensions 7-zip open with send to" },

  // ---- Presets and profiles ----
  { key: "presets.ui", section: "Presets and profiles", label: "Interface style", description: "The whole look in one step, in the style of a system you know: layout, colors, icons and shortcuts.", control: { type: "uiPresets" }, keywords: "preset windows mac macos linux finder explorer layout look style" },
  { key: "presets.performance", section: "Presets and profiles", label: "Performance preset", description: "Steps from the lightest to the heaviest, each with the kind of device it suits. Phones and tablets get their own, much lighter, set.", control: { type: "perfPresets" }, keywords: "preset barebones feather reduced acceptable average satisfactory decent performance power excelling exceptional godlike speed memory" },
  { key: "presets.profiles", section: "Presets and profiles", label: "Your profiles", description: "Saved copies of all your settings, as many as you want.", control: { type: "profiles" }, keywords: "profile save switch settings backup work laptop" },

  // ---- Appearance ----
  { key: "appearance.accent", section: "Appearance", label: "Accent color", control: { type: "color" } },
  { key: "appearance.density", section: "Appearance", label: "Row density", description: "Compact fits as many rows as Windows 10 did.", control: { type: "select", options: [{ value: "compact", label: "Compact" }, { value: "comfortable", label: "Comfortable" }] }, keywords: "spacing size" },
  { key: "appearance.viewLevel", section: "Appearance", label: "Folder view", description: "Ctrl + scroll over a folder changes this too. It is remembered and used in every folder (Advanced > Files and lists > Use the same view in every folder).", control: { type: "select", options: [{ value: "0", label: "Details" }, { value: "1", label: "List" }, { value: "2", label: "Small icons" }, { value: "3", label: "Medium icons" }, { value: "4", label: "Large icons" }, { value: "5", label: "Extra large icons" }] }, keywords: "icons thumbnails details list size" },
  { key: "appearance.showDriveSpace", section: "Appearance", label: "Show drive space on This PC", description: "Free or used space under every drive (choose which on the This PC page).", control: toggle, keywords: "storage capacity" },
  { key: "appearance.sidebarDriveSpace", section: "Appearance", label: "Show drive space in the navigation pane", description: "Off by default. A thin bar under each drive in the Drives list, filled with the space used or the space free (whichever is chosen on the This PC page, or just below), and the figure beside the name.", control: toggle, keywords: "storage capacity sidebar left bar free used" },
  { key: "appearance.showLocalDrives", section: "Appearance", label: "Show local drives", description: "The drives inside or plugged into this computer, in the navigation pane and on This PC. Turn this and the next one off to hide all drives.", control: toggle, keywords: "internal disks usb sidebar this pc hide" },
  { key: "appearance.showNetworkDrives", section: "Appearance", label: "Show network drives", description: "Mapped drives and network locations, in the navigation pane and on This PC.", control: toggle, keywords: "shares unc smb nas sidebar this pc hide" },
  { key: "appearance.hideNetworkAddress", section: "Appearance", label: "Hide server addresses and IPs", description: "On by default. Hides hosts, IP addresses and share paths for network drives, servers and shares: \"games (S:)\" instead of \"games (\\\\192.168.1.5) (S:)\". Applies to names, tooltips, tabs, breadcrumbs and details. A drive can override this from its right-click menu. The address bar shows the real address while you are editing it.", control: toggle, keywords: "ip network share unc" },
  { key: "appearance.showDetailsPane", section: "Appearance", label: "Show the details pane", control: toggle, keywords: "properties preview" },
  { key: "appearance.driveChart", section: "Appearance", label: "Drive space display", control: { type: "select", options: [{ value: "bar", label: "Bars" }, { value: "donut", label: "Rings" }] }, keywords: "pie donut chart this pc" },
  { key: "appearance.driveMetric", section: "Appearance", label: "Drive space shows", control: { type: "select", options: [{ value: "used", label: "Space used" }, { value: "free", label: "Space free" }] } },

  // ---- Themes ----
  { key: "appearance.theme", section: "Themes", label: "Color theme", description: "Built-in color schemes, including Windows 11, Windows 10, macOS and GNOME looks. The accent color in Appearance overrides a theme's own accent once you change it.", control: { type: "themes" }, keywords: "dark light colors scheme windows 10 11" },
  { key: "appearance.style.font", section: "Themes", label: "Font", description: "Built-in stacks, or a font file you import below.", control: { type: "font" }, keywords: "typeface text" },
  { key: "font.areas", section: "Themes", label: "Fonts for each part", description: "Give the file list, navigation pane, tabs and toolbar, paths and code, and the terminal a font of their own.", control: { type: "fontAreas" }, keywords: "typeface font family installed system terminal sidebar file list" },
  { key: "appearance.style.fontSize", section: "Themes", label: "Text size", control: { type: "number", min: 10, max: 22, unit: "px" } },
  { key: "appearance.style.radius", section: "Themes", label: "Corner roundness", control: { type: "number", min: 0, max: 20, unit: "px" }, keywords: "rounded square corners" },
  { key: "appearance.style.menu", section: "Themes", label: "Menu style", control: { type: "select", options: [{ value: "glass", label: "Glass (blurred)" }, { value: "flat", label: "Flat" }, { value: "outlined", label: "Outlined" }, { value: "floating", label: "Floating card" }] }, keywords: "context menu popup" },
  { key: "appearance.style.icons", section: "Themes", label: "Icon style", control: { type: "select", options: [{ value: "fluent", label: "Fluent (open-source, Windows 11 style)" }, { value: "outline", label: "Outline" }, { value: "bold", label: "Bold" }, { value: "thin", label: "Thin" }] }, keywords: "toolbar glyphs windows 11 fluent" },
  { key: "appearance.style.folderColor", section: "Themes", label: "Folder color", control: { type: "select", options: [{ value: "classic", label: "Classic yellow" }, { value: "blue", label: "Blue" }, { value: "mono", label: "Grey" }, { value: "accent", label: "Match accent" }] }, keywords: "folder icon" },
  { key: "appearance.style.background", section: "Themes", label: "Window background", control: { type: "select", options: [{ value: "plain", label: "Plain" }, { value: "gradient", label: "Soft gradient" }, { value: "aurora", label: "Aurora" }, { value: "dusk", label: "Dusk" }, { value: "mist", label: "Mist" }, { value: "image", label: "My picture (import below)" }] }, keywords: "wallpaper backdrop" },
  { key: "appearance.style.imports", section: "Themes", label: "Import fonts, style sheets and pictures", description: "Files are copied into the app's own folder. A style sheet is plain CSS that can override any variable or class (see docs/THEMING.md).", control: { type: "imports" }, keywords: "custom font ttf otf woff css background image" },
  { key: "icons.panel", section: "Appearance", label: "Icons", description: "Swap any icon in the app for another app icon or a picture of your own, and put each back to its default. A changed icon changes everywhere it is used.", control: { type: "icons" }, keywords: "icon change custom picture drive folder image replace reset default" },
  { key: "appearance.customCss", section: "Themes", label: "Custom CSS", description: "Applied on top of the built-in theme as you type. Every color, radius and size is a CSS variable; see docs/THEMING.md for the full list and the app's class names.", control: { type: "css" }, keywords: "theme style skin stylesheet" },

  // ---- Layout ----
  { key: "appearance.layout", section: "Layout", label: "Window layout", description: "Every part of the window is a panel: dock it to any edge, float it, make it pop out from a button, collapse it or hide it.", control: { type: "layout" }, keywords: "dock move drag panels toolbar tabs sidebar position" },

  // ---- Navigation pane ----
  { key: "appearance.showFrequent", section: "Navigation pane", label: "Show Frequently used", description: "The folders you open most. Off hides the folders and the heading text as well.", control: toggle },
  { key: "appearance.showPinned", section: "Navigation pane", label: "Show Pinned", control: toggle },
  { key: "appearance.showDrives", section: "Navigation pane", label: "Show Drives", control: toggle },
  { key: "appearance.sidebarCollapsed", section: "Navigation pane", label: "Collapse to icons only", description: "Drag the pane's edge to resize it, or drag it nearly shut.", control: toggle },
  { key: "appearance.sidebarWidth", section: "Navigation pane", label: "Width", control: { type: "number", min: 150, max: 480, step: 10, unit: "px" } },

  // ---- Performance ----
  { key: "performance.resources", section: "Performance", label: "What the app is using", description: "Live processor, memory and disk use, what is kept for later, and ways to use less.", control: { type: "resources" }, keywords: "cpu ram memory disk usage monitor footprint compress shrink release" },
  { key: "performance.priority", section: "Performance", label: "Resource use", description: "Automatic adapts threads, caches and background work to your CPU, memory and battery. Manual uses the limits below exactly.", control: { type: "select", options: [{ value: "dynamic", label: "Automatic" }, { value: "manual", label: "Manual" }] }, keywords: "priority dynamic battery cpu ram" },
  { key: "performance.maxThreads", section: "Performance", label: "Maximum threads", description: "0 uses one per CPU core.", control: { type: "number", min: 0, max: 128 }, keywords: "cpu cores parallel" },
  { key: "performance.maxCpuPercent", section: "Performance", label: "Processor limit", description: "The most of your processor the app may use, as a share of all its cores. It limits how many worker threads run at once. 0 means no limit. Each performance preset sets this; God-like sets none.", control: { type: "number", min: 0, max: 100, step: 5, unit: "%" }, keywords: "cpu cap limit threads share percent" },
  { key: "performance.maxMemoryPercent", section: "Performance", label: "Memory limit", description: "The most of your computer's memory the app's caches may hold, as a share of what is installed. 0 means no limit. Each performance preset sets this; God-like sets none.", control: { type: "number", min: 0, max: 100, step: 1, unit: "%" }, keywords: "ram memory cap limit percent" },
  { key: "performance.maxCacheMb", section: "Performance", label: "Maximum memory for caches", description: "Upper limit for the file index and cached folder listings, in megabytes. 0 means no limit. The lower of this and the memory limit applies.", control: { type: "number", min: 0, max: 65536, step: 16, unit: "MB" }, keywords: "ram memory" },
  { key: "performance.backgroundMode", section: "Performance", label: "When minimized or in the background", description: "Cuts the app's work while it is out of your way. Aggressive: one thread, no thumbnails, indexing or preloading, a small cache, and slow progress updates, as soon as the window is minimized, hidden or has been behind other windows for a few seconds. Gentle: halves the work, only while minimized or hidden. Running file transfers are never slowed down. Changing this yourself keeps performance presets from changing it.", control: { type: "select", options: [{ value: "aggressive", label: "Aggressive" }, { value: "gentle", label: "Gentle" }, { value: "off", label: "Off (keep working as usual)" }] }, keywords: "minimize minimised background idle battery save power cpu hidden" },
  { key: "performance.backgroundLocked", section: "Performance", label: "Keep my background setting when I apply a preset", description: "On: performance presets leave the setting above alone. This turns on by itself when you change it. Turn it off to let presets set it again.", control: toggle, keywords: "preset lock override" },
  {
    key: "performance.renderBackend",
    section: "Performance",
    label: "Native graphics backend",
    description: "Which native graphics API the window uses to render lists, icons, blur and media layers. Automatic picks the strongest hardware API for this system and tier (DirectX 12/11 on Windows, Metal on Mac, Vulkan/OpenGL on Linux). Requires a restart to apply.",
    control: {
      type: "select",
      options: [
        { value: "auto", label: "Automatic (native best for tier)" },
        { value: "directx12", label: "DirectX 12 (Windows)" },
        { value: "directx11", label: "DirectX 11 (Windows)" },
        { value: "vulkan", label: "Vulkan" },
        { value: "opengl", label: "OpenGL" },
        { value: "metal", label: "Metal (macOS)" },
        { value: "software", label: "Software (CPU only, lowest GPU use)" },
      ],
    },
    keywords: "gpu directx dx12 dx11 d3d11 d3d12 vulkan metal opengl rendering backend native",
  },
  { key: "performance.hardwareAcceleration", section: "Performance", label: "GPU hardware rendering", description: "Uses your graphics card instead of CPU software rasterization for compositing and drawing. Takes effect on restart.", control: toggle, keywords: "gpu hardware acceleration native render" },
  { key: "performance.gpuRasterization", section: "Performance", label: "GPU layer rasterization", description: "Rasters tiles, text, shadows and thumbnails directly on the GPU compositor across multiple raster threads. Takes effect on restart.", control: toggle, keywords: "gpu rasterize skia directx vulkan" },
  { key: "performance.zeroCopy", section: "Performance", label: "Zero-copy GPU textures", description: "Streams decoded thumbnail and image textures directly into native GPU memory buffers without extra copies. Takes effect on restart.", control: toggle, keywords: "zero copy texture vram gpu" },
  { key: "performance.vsync", section: "Performance", label: "Synchronize frames with display (VSync)", description: "On: matches your monitor refresh rate cleanly. Off: unlocks frame rate and disables VSync stalls for minimal input/scroll latency on high tiers (takes effect on restart).", control: toggle, keywords: "vsync fps frame rate latency uncapped godlike" },
  { key: "performance.processPriority", section: "Performance", label: "System scheduling priority", description: "Native OS process priority class for directory reads, thumbnail decoding and search workers. Automatic raises priority while in use on high performance presets.", control: { type: "select", options: [{ value: "auto", label: "Automatic (follows the preset tier)" }, { value: "low", label: "Low (yields to everything else)" }, { value: "normal", label: "Normal" }, { value: "high", label: "High (Above Normal while active)" }] }, keywords: "priority cpu scheduling os native thread process" },
  { key: "performance.renderingLocked", section: "Performance", label: "Keep my rendering and priority settings when I apply a preset", description: "On: switching performance presets will not override your graphics backend, GPU pipeline options, VSync or process priority. Turns on automatically when you customize any of them.", control: toggle, keywords: "preset lock override render gpu" },
  { key: "performance.reduceOnBattery", section: "Performance", label: "Do less on battery", description: "Pauses background indexing and uses fewer threads when unplugged.", control: toggle, keywords: "laptop power" },
  { key: "performance.backgroundIndexing", section: "Performance", label: "Index files in the background", description: "Keeps search instant. Without it, searches scan the folder each time.", control: toggle, keywords: "cache search" },
  { key: "performance.indexDrives", section: "Performance", label: "Index all local drives", description: "Every local drive is indexed in the background so search answers instantly. Turn off to index only your home folder. Network shares are never indexed automatically.", control: toggle },
  { key: "performance.metadataCaching", section: "Performance", label: "Cache folder listings", description: "Folders you revisit appear instantly and refresh in the background.", control: toggle, keywords: "metadata cache" },
  { key: "performance.predictivePrefetch", section: "Performance", label: "Preload folders you are likely to open", description: "Loads a folder when you point at it, and the ones you usually open next.", control: toggle, keywords: "predict prefetch hover" },
  { key: "performance.lazyLoading", section: "Performance", label: "Lazy rendering", description: "Only draws rows near the screen. Turning this off renders further ahead and uses more memory.", control: toggle, keywords: "virtual scroll" },
  { key: "performance.thumbnails", section: "Thumbnails", label: "Show thumbnails", description: "The master switch. Off: every file keeps its type icon. Each performance level sets it.", control: toggle, keywords: "preview pictures photos videos documents" },
  { key: "performance.thumbnailSize", section: "Thumbnails", label: "Preview size in the details pane and elsewhere", description: "How large previews are made when no icon size applies.", control: { type: "number", min: 48, max: 256, step: 16, unit: "px" } },

  // ---- Storage ----
  { key: "logs.console", section: "Logs", label: "Activity log", description: "Every action the app takes, as it happens, with how long it took and whether it stopped making progress (HUNG). Save it as a text file to send along with a bug report.", control: { type: "activityLog" }, keywords: "log console hung stall freeze debug events activity trace diagnostics" },
  { key: "storage.caches", section: "Storage", label: "What the app stores", description: "Each kind of cache, how much space it takes, and a button to clear just that one.", control: { type: "storage" }, keywords: "cache clear thumbnails index disk usage snapshots history space" },

  // ---- Updates ----
  { key: "updates.panel", section: "Updates", label: "Version and updates", description: "Which build this is, whether a newer one is out, and a button to install it.", control: { type: "updates" }, keywords: "update upgrade version latest download install restart check" },
  { key: "updates.enabled", section: "Updates", label: "Check for updates", description: "Looks at the builds published on GitHub. Nothing else is sent anywhere, and nothing is installed without the steps below.", control: toggle, keywords: "auto update automatic" },
  { key: "updates.checkOnOpen", section: "Updates", label: "Check when the app opens", control: toggle, keywords: "startup launch" },
  { key: "updates.background", section: "Updates", label: "Keep checking while the app is open", description: "Looks again every few hours. Paused while the window is minimized and the background saver is on.", control: toggle, keywords: "background periodic" },
  { key: "updates.checkHours", section: "Updates", label: "Hours between checks", control: { type: "number", min: 1, max: 168, unit: "h" }, keywords: "interval frequency" },
  { key: "updates.autoDownload", section: "Updates", label: "Download new builds automatically", description: "The download appears with your other transfers. When it is done you are asked whether to restart now or later; nothing restarts by itself. Off: use the Update button above.", control: toggle, keywords: "auto update download install background" },
  { key: "updates.notify", section: "Updates", label: "Tell me when a new build is out", description: "A short note in the notifications, once for each new build.", control: toggle, keywords: "notification alert announce" },
  { key: "updates.includeUnreleased", section: "Updates", label: "Also offer unreleased builds", description: "Off (default): only versions published under the repository's Releases are offered, and they download straight from GitHub. On: builds from the repository's automated runs are offered too when they are newer. GitHub only lets signed-in accounts download those, so they come through the third-party site nightly.link unless you add an access token below. Some antivirus programs flag that site.", control: toggle, keywords: "nightly artifacts actions beta prerelease antivirus" },
  { key: "updates.repo", section: "Updates", label: "Repository", description: "owner/name on GitHub whose builds are used.", control: { type: "text", placeholder: "enVIdPluginSuite/file-explorer" }, keywords: "github source fork" },
  { key: "updates.branch", section: "Updates", label: "Branch", description: "Only builds made from this branch are offered.", control: { type: "text", placeholder: "main" }, keywords: "github channel" },
  { key: "updates.token", section: "Updates", label: "GitHub access token (optional)", description: "GitHub lets only signed-in accounts download builds from its Actions page. With a token that may read Actions the app downloads from GitHub itself. Without one it uses nightly.link, which works only for repositories that have it installed. The token stays on this computer and is never part of exports or profiles.", control: { type: "text", secret: true, placeholder: "github_pat_…" }, keywords: "pat key secret password nightly" },

  // ---- File transfers ----
  { key: "transfer.conflict", section: "File transfers", label: "When a file already exists", description: "Ask me shows, for each file that exists on both sides, how the two compare and lets you Replace, Skip or Keep both, for that file or for all that follow. Folders with the same name are merged, and the files inside are asked about one by one.", control: { type: "select", options: [{ value: "ask", label: "Ask me" }, { value: "keepBoth", label: "Keep both" }, { value: "overwrite", label: "Replace" }, { value: "skip", label: "Skip" }] }, keywords: "conflict duplicate overwrite" },
  { key: "transfer.maxParallel", section: "File transfers", label: "Parallel file streams", description: "0 lets the app choose per job from file sizes and drive types.", control: { type: "number", min: 0, max: 64 }, keywords: "threads workers" },
  { key: "transfer.largeFileThresholdMb", section: "File transfers", label: "Treat files as large from", description: "Large files are copied a few at a time with big buffers; smaller ones in parallel.", control: { type: "number", min: 1, max: 4096, unit: "MB" } },
  { key: "transfer.preserveTimestamps", section: "File transfers", label: "Keep original modified dates", control: toggle, keywords: "timestamps mtime" },
  { key: "transfer.rememberDeviceSpeeds", section: "File transfers", label: "Remember drive speeds", description: "Uses past transfers between the same drives to estimate time accurately from the first second.", control: toggle, keywords: "eta estimate" },
  { key: "transfer.historyLimit", section: "File transfers", label: "Transfers to remember", description: "A few bytes each. The most recent ones teach the app how long similar copies take. 0 remembers none; clear them in Storage.", control: { type: "number", min: 0, max: 5000, step: 50 }, keywords: "history prediction model" },
  { key: "transfer.method", section: "File transfers", label: "Copy method", description: "Automatic uses a tool (rclone, Robocopy or rsync) for many small files and the built-in engine for big ones, then learns from your own transfers which method is fastest for each pair of drives. The two rules can be changed under Advanced. SMB-tuned and NFS-tuned only apply when a network share of that kind is involved; rclone only when it is installed. A method that cannot run on a particular job falls back to the built-in engine.", control: { type: "select", options: [{ value: "auto", label: "Automatic (learn from results)" }, { value: "native", label: "Built-in engine" }, { value: "robocopy", label: "Robocopy (Windows)" }, { value: "rsync", label: "rsync (macOS / Linux)" }, { value: "smb", label: "SMB-tuned (Windows shares; Robocopy with compression)" }, { value: "nfs", label: "NFS-tuned (rsync, whole-file)" }, { value: "rclone", label: "rclone (if installed)" }] }, keywords: "robocopy rsync rclone smb nfs algorithm engine network share" },
  { key: "transfer.tryOtherMethods", section: "File transfers", label: "Try other methods on big copies", description: "Lets a large copy now and then use another available method so the app can measure which is faster on your drives. Off keeps everything on the built-in engine.", control: toggle, keywords: "learn explore robocopy rsync" },
  { key: "transfer.maxSpeedKb", section: "File transfers", label: "Maximum total speed", description: "The most all transfers together may move, in KB per second (1024 KB/s = 1 MB/s). 0 means no limit. A transfer held to a limit uses the built-in engine, since copy tools cannot be held to one.", control: { type: "number", min: 0, max: 100_000_000, step: 256, unit: "KB/s" }, keywords: "bandwidth throttle limit speed network cap download" },
  { key: "transfer.targetSpeedKb", section: "File transfers", label: "Target speed for each transfer", description: "Every new transfer is held to this speed, in KB per second. 0 means none. You can change it for one transfer while it runs, in the expanded Transfers window.", control: { type: "number", min: 0, max: 100_000_000, step: 256, unit: "KB/s" }, keywords: "bandwidth throttle individual per transfer speed" },
  { key: "transfer.maxMinutes", section: "File transfers", label: "Longest a transfer may run", description: "After this many minutes of work a transfer pauses or stops (next setting). 0 means no limit.", control: { type: "number", min: 0, max: 10080, step: 5, unit: "min" }, keywords: "time limit timeout duration" },
  { key: "transfer.onTimeLimit", section: "File transfers", label: "When the time limit is reached", description: "Pausing keeps your place so you can resume; stopping ends the transfer and keeps what was copied.", control: { type: "select", options: [{ value: "pause", label: "Pause it" }, { value: "stop", label: "Stop it" }] }, keywords: "timeout" },
  { key: "transfer.defaultPriority", section: "File transfers", label: "Priority of new transfers", description: "High transfers go first when others are waiting. Low ones go last and give way to other programs on your computer. Change it for one transfer while it runs.", control: { type: "select", options: [{ value: "high", label: "High" }, { value: "normal", label: "Normal" }, { value: "low", label: "Low" }] }, keywords: "priority importance order queue" },
  { key: "transfer.maxActive", section: "File transfers", label: "Transfers at the same time", description: "How many transfers work at once; the rest wait in a queue, highest priority first, and can be started early. 0 runs them all at once.", control: { type: "number", min: 0, max: 32 }, keywords: "queue simultaneous concurrent" },
  { key: "transfer.sameDiskTurns", section: "File transfers", label: "Hard disk: one transfer at a time", description: "Transfers on different drives, SSDs and network shares always work at the same time. Two transfers that use the same spinning hard disk go faster one after the other, because the disk wastes its time moving between them; with this on the second starts when the first is done, or at once with Start now. Off lets them work together anyway.", control: toggle, keywords: "concurrent simultaneous parallel paste hdd queue" },
  { key: "transfer.progressIntervalMs", section: "File transfers", label: "Progress update rate", description: "Milliseconds between progress updates. 0 chooses by power: 100 on desktops, 250 on laptops and mid-range machines, 500 on weak ones or low battery.", control: { type: "number", min: 0, max: 2000, step: 50, unit: "ms" }, keywords: "polling refresh rate interval" },

  // ---- Search ----
  { key: "search.scopeToCurrentFolder", section: "Search", label: "Search inside the current folder", description: "Turn off to search everything indexed, wherever you are.", control: toggle, keywords: "scope" },
  { key: "general.openSearchResultInNewTab", section: "Search", label: "Open results in a new tab", description: "Keeps the search results available in their own tab.", control: toggle },
  { key: "search.includeHidden", section: "Search", label: "Include hidden items in results", control: toggle },
  { key: "search.limit", section: "Search", label: "Maximum results", control: { type: "number", min: 10, max: 10000, step: 10 } },

  // ---- Privacy and locks ----
  { key: "privacy.locks", section: "Privacy and locks", label: "Locked folders and drives", description: "Passcodes that keep folders and drives closed inside this app.", control: { type: "locks" }, keywords: "passcode password lock protect private secure hide" },

  { key: "privacy.deleteGuard", section: "Privacy and locks", label: "Password for deleting for good", description: "Off by default. Asks for a password before anything is deleted permanently, so nothing goes by accident.", control: { type: "deleteGuard" }, keywords: "password permanent delete shift delete confirm protect accident empty trash recycle bin" },
  { key: "privacy.sshLogins", section: "Privacy and locks", label: "Save SSH keys and logins between sessions", description: "Off by default (highly recommended to leave off). Optionally saves SSH key file paths, usernames, servers and passphrases across restarts.", control: { type: "sshLogins" }, keywords: "ssh sftp key passphrase password login save remember credentials server" },
  { key: "privacy.mac", section: "macOS permissions", label: "Full Disk Access and folder permissions", description: "One switch so macOS stops asking folder by folder, whether it is working, and what the app may search in the background.", control: { type: "macAccess" }, os: ["macos"], keywords: "permission full disk access mac macos privacy allow folders desktop documents downloads prompt" },

  // ---- Right-click menu ----
  { key: "contextMenu.hidden", section: "Right-click menu", label: "Entries shown in the right-click menu", control: { type: "menuItems" }, keywords: "context menu customize" },

  // ---- Terminal ----
  { key: "terminal.windowsShell", section: "Terminal", label: "Terminal to open", description: "Windows Terminal, WSL and Git Bash must be installed to be used.", control: { type: "select", options: [{ value: "cmd", label: "Command Prompt" }, { value: "powershell", label: "Windows PowerShell" }, { value: "pwsh", label: "PowerShell 7" }, { value: "wt", label: "Windows Terminal" }, { value: "wsl", label: "WSL (Linux)" }, { value: "gitbash", label: "Git Bash" }] }, os: ["windows"], keywords: "cmd command prompt shell powershell pwsh wsl git bash" },
  { key: "terminal.windowsProfile", section: "Terminal", label: "Windows Terminal profile", description: "The profile to open, by its name in Windows Terminal (for example Ubuntu). Empty uses its default. Only used with Windows Terminal.", control: { type: "text", placeholder: "Default profile" }, os: ["windows"], keywords: "wt profile ubuntu" },
  { key: "terminal.macTerminal", section: "Terminal", label: "Terminal app", description: "Terminal and iTerm can also run a startup command.", control: { type: "text", placeholder: "Terminal" }, os: ["macos"], keywords: "iterm shell" },
  { key: "terminal.linuxTerminal", section: "Terminal", label: "Terminal program", description: "Leave empty to detect one automatically.", control: { type: "text", placeholder: "e.g. konsole" }, os: ["linux"], keywords: "shell gnome-terminal" },
  { key: "terminal.shellProgram", section: "Terminal", label: "Shell to run inside it", description: "bash, zsh, fish or a full path. Empty uses your login shell.", control: { type: "text", placeholder: "Login shell" }, os: ["linux", "macos"], keywords: "bash zsh fish sh" },
  { key: "terminal.startupCommand", section: "Terminal", label: "Run when it opens", description: "A command to run as soon as the terminal opens, for example git status or ls -la. The terminal stays open afterwards. Not used for administrator terminals.", control: { type: "text", placeholder: "Nothing" }, keywords: "startup command run script default init" },
  { key: "terminal.newTab", section: "Terminal", label: "Open in a new tab", description: "Add a tab to the terminal window that is already open, where the terminal supports it (Windows Terminal, GNOME Terminal, Ptyxis, Konsole).", control: toggle, keywords: "tab window reuse" },
  { key: "terminal.defaultFolder", section: "Terminal", label: "Open terminal in", description: "Which folder the Open terminal button and shortcut use. The menu next to it can still pick another.", control: { type: "select", options: [{ value: "here", label: "The folder being viewed" }, { value: "home", label: "Your home folder" }] }, keywords: "start directory working folder" },
  { key: "terminal.useSelectedFolder", section: "Terminal", label: "Use the selected folder", description: "When exactly one folder is selected, open the terminal inside it.", control: toggle, keywords: "selection selected directory" },
  { key: "terminal.customCommand", section: "Terminal", label: "Custom command", description: "Replaces all the choices above with a command of your own. {path} stands for the folder, for example: alacritty --working-directory {path}. Leave empty to use the choices above.", control: { type: "text", placeholder: "Use the choices above" }, keywords: "custom launch command template path placeholder" },
];

/** Advanced options appear in Settings like any other entry (and in search). */
const SHOWN_IN_GENERAL = ["files.singleClickFolders", "files.checkboxes", "files.rememberOpenWith"];
const SHOWN_IN_FOLDER_VIEWS = ["files.scope.sort", "files.scope.filter", "files.scope.columns", "files.scope.view", "files.sortKey", "files.sortAsc", "files.rememberSort", "files.keepFilter"];
const SHOWN_IN_PERFORMANCE = ["perf.uiUnload", "perf.shellMenuCache", "perf.shellMenuPrefetch", "perf.loadRefresh", "perf.compactWhenIdle"];
const sectionOf = (d: (typeof ADVANCED)[number]) =>
  SHOWN_IN_PERFORMANCE.includes(d.key) ? "Performance" : SHOWN_IN_GENERAL.includes(d.key) ? "General" : SHOWN_IN_FOLDER_VIEWS.includes(d.key) ? "Folder views" : d.group === "Terminal panel" ? "Terminal" : d.group === "Logs" ? "Logs" : d.key.startsWith("cache.") ? "Storage" : d.key.startsWith("locks.") ? "Privacy and locks" : d.group === "Notifications" || d.group === "Navigation pane" || d.group === "General" || d.group === "Thumbnails" ? d.group : ["tabs.width", "tabs.minWidth", "tabs.height", "ui.scale", "ui.iconSize", "ui.topBarScale"].includes(d.key) ? "Appearance" : "Advanced";

for (const d of ADVANCED) {
  SETTINGS.push({ key: `advanced.${d.key}`, section: sectionOf(d), label: d.label, description: d.description, control: d.control as Control, keywords: `${d.group} advanced` });
}

export function getSetting(settings: Settings, key: string): unknown {
  if (key.startsWith("advanced.")) return adv(settings, key.slice("advanced.".length));
  const v = key.split(".").reduce<unknown>((o, k) => (o as Record<string, unknown>)?.[k], settings);
  // The view level is stored as a number but edited as a drop-down of strings.
  return key === "appearance.viewLevel" ? String(v) : v;
}

export function setSetting(settings: Settings, key: string, value: unknown): void {
  if (key.startsWith("advanced.")) {
    settings.advanced = { ...settings.advanced, [key.slice("advanced.".length)]: value };
    return;
  }
  const parts = key.split(".");
  const last = parts.pop()!;
  const target = parts.reduce<Record<string, unknown>>((o, k) => o[k] as Record<string, unknown>, settings as unknown as Record<string, unknown>);
  target[last] = key === "appearance.viewLevel" ? Number(value) : value;
}
