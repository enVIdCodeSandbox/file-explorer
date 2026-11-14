// The phone and tablet layout: one column, a bar on top, and (in two of the
// three looks) a bar along the bottom. The folder view, search, settings,
// transfers and every dialog are the same components the computer layout uses;
// this file only arranges them for a narrow touch screen.
//
// Three arrangements, each like the file manager that comes with a phone:
//   google   Files by Google: search pill, Recents / Browse along the bottom
//   samsung  My Files: one home page with a large title, categories and cards
//   apple    Files: large titles, grouped lists, Recents / Browse along the bottom

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "../api";
import { adv } from "../settings/advanced";
import { useStore, VIEW_LEVELS, type SortKey } from "../store";
import { usePhoneChrome, type PhoneChrome } from "../util/chrome";
import { askStorageAccess, colorSystemBars, storageAccess } from "../util/device";
import { driveName } from "../util/drives";
import { formatDate, formatSize, kindOf } from "../util/format";
import { HOME, THIS_PC, analyzeLocation, baseName, isArchiveLoc, isRemote, parseLocation, remoteLabel, searchLocation } from "../util/path";
import { phoneCrumbs, phoneParent, phoneTitle, type Root } from "../util/phonePath";
import { viewLevelOf } from "../util/viewLevel";
import { openFile } from "../store";
import { AnalyzerView } from "./Analyzer";
import { serverUrl } from "./Chrome";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { FileView } from "./FileView";
import { FileIcon, Icon } from "./Icons";
import { SearchView } from "./Views";

/** The storages as the phone names them, for breadcrumbs and "up". */
function useRoots(): Root[] {
  const drives = useStore((s) => s.drives);
  const appearance = useStore((s) => s.settings?.appearance);
  return useMemo(() => drives.map((d) => ({ name: appearance ? driveName(d, appearance) : d.name, mount: d.mount })), [drives, appearance]);
}

const SORTS: [SortKey, string][] = [
  ["name", "Name"],
  ["modified", "Date"],
  ["size", "Size"],
  ["type", "Type"],
];

/** Where a category button goes, and its color in the looks that color them. */
const CATEGORIES: { label: string; kind: string; color: string }[] = [
  { label: "Downloads", kind: "download", color: "#3b82f6" },
  { label: "Pictures", kind: "image", color: "#22c55e" },
  { label: "Camera", kind: "image", color: "#14b8a6" },
  { label: "Videos", kind: "video", color: "#a855f7" },
  { label: "Music", kind: "audio", color: "#f97316" },
  { label: "Documents", kind: "document", color: "#eab308" },
  { label: "Screenshots", kind: "image", color: "#ec4899" },
];
const CATEGORY_ICON: Record<string, string> = { download: "expand", image: "grid", video: "play", audio: "clock", document: "list" };

// ---------------------------------------------------------------------------
// Root pages: Recents and Browse (one page in the Samsung look)
// ---------------------------------------------------------------------------

function Section({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="m-section">
      <header>
        <h2>{title}</h2>
        {action}
      </header>
      {children}
    </section>
  );
}

function Recents({ limit }: { limit: number }) {
  const recent = useStore((s) => s.recent);
  const { navigate, addRecent } = useStore.getState();
  if (!recent.length) return <p className="m-empty">Files and folders you open will be listed here.</p>;
  return (
    <div className="m-list">
      {recent.slice(0, limit).map((r) => (
        <button
          key={r.path}
          className="m-item"
          onClick={() => {
            if (r.isDir) return navigate(r.path);
            addRecent(r.path, false);
            void openFile(r.path);
          }}
        >
          <span className="m-item-icon"><FileIcon kind={kindOf(r.path, r.isDir)} name={baseName(r.path)} size={30} /></span>
          <span className="m-item-text">
            <strong>{baseName(r.path)}</strong>
            <span>{formatDate(r.at)}</span>
          </span>
        </button>
      ))}
    </div>
  );
}

function Browse({ chrome }: { chrome: PhoneChrome }) {
  const drives = useStore((s) => s.drives);
  const places = useStore((s) => s.places);
  const pins = useStore((s) => s.pins);
  const servers = useStore((s) => s.servers);
  const appearance = useStore((s) => s.settings!.appearance);
  const { navigate, openConnect, openSettings } = useStore.getState();
  const categories = CATEGORIES.map((c) => ({ ...c, path: places.find((p) => p.label === c.label)?.path })).filter((c) => c.path);
  const home = places.find((p) => p.label === "Home")?.path ?? drives[0]?.mount;
  const names = { storage: chrome === "apple" ? "Locations" : "Storage", pins: chrome === "apple" ? "Favorites" : chrome === "samsung" ? "Favorites" : "Starred", servers: chrome === "apple" ? "Shared" : "Network storage" };

  return (
    <>
      {categories.length > 0 && (
        <Section title="Categories">
          <div className="m-categories">
            {categories.map((c) => (
              <button key={c.label} className="m-category" onClick={() => navigate(c.path!)} style={{ "--cat": c.color } as React.CSSProperties}>
                <span className="m-category-icon"><Icon name={CATEGORY_ICON[c.kind] ?? "folderOpen"} size={22} /></span>
                <span>{c.label}</span>
              </button>
            ))}
          </div>
        </Section>
      )}

      <Section title={names.storage}>
        <div className="m-list">
          {drives.length === 0 && <p className="m-empty">No storage was found.</p>}
          {drives.map((d) => {
            const used = d.total > 0 ? (d.total - d.available) / d.total : 0;
            return (
              <button key={d.mount} className="m-item" onClick={() => navigate(d.mount)}>
                <span className="m-item-icon" data-tint><Icon name={d.media === "network" ? "network" : d.removable ? "archive" : "drive"} size={22} /></span>
                <span className="m-item-text">
                  <strong>{driveName(d, appearance)}</strong>
                  {d.total > 0 ? (
                    <>
                      <span>{formatSize(d.total - d.available)} used of {formatSize(d.total)}</span>
                      <span className="m-meter" aria-hidden="true"><i style={{ width: `${Math.round(used * 100)}%` }} data-full={used > 0.9 || undefined} /></span>
                    </>
                  ) : (
                    <span>{d.mount}</span>
                  )}
                </span>
                <Icon name="chevron" size={14} />
              </button>
            );
          })}
        </div>
      </Section>

      {pins.length > 0 && (
        <Section title={names.pins}>
          <div className="m-list">
            {pins.map((p) => (
              <button key={p.path} className="m-item" onClick={() => (p.isFile ? void openFile(p.path) : navigate(p.path))}>
                <span className="m-item-icon"><FileIcon kind={p.isFile ? kindOf(p.label, false) : "folder"} name={p.isFile ? p.label : undefined} size={30} /></span>
                <span className="m-item-text"><strong>{p.label}</strong></span>
                <Icon name="chevron" size={14} />
              </button>
            ))}
          </div>
        </Section>
      )}

      <Section title={names.servers}>
        <div className="m-list">
          {servers.map((s) => (
            <button key={`${s.protocol}-${s.host}-${s.user}-${s.path}`} className="m-item" onClick={() => navigate(serverUrl(s))}>
              <span className="m-item-icon" data-tint><Icon name="network" size={22} /></span>
              <span className="m-item-text">
                <strong>{s.name || s.host}</strong>
                <span>{s.protocol.toUpperCase()}</span>
              </span>
              <Icon name="chevron" size={14} />
            </button>
          ))}
          <button className="m-item" onClick={() => openConnect()}>
            <span className="m-item-icon" data-tint><Icon name="plus" size={22} /></span>
            <span className="m-item-text">
              <strong>Connect to a server</strong>
              <span>FTP, SFTP or WebDAV</span>
            </span>
          </button>
        </div>
      </Section>

      <Section title={chrome === "samsung" ? "Utilities" : "Tools"}>
        <div className="m-list">
          {home && (
            <button className="m-item" onClick={() => navigate(analyzeLocation(home))}>
              <span className="m-item-icon" data-tint><Icon name="chart" size={22} /></span>
              <span className="m-item-text">
                <strong>{chrome === "samsung" ? "Analyse storage" : "Analyze storage"}</strong>
                <span>See what takes up the space</span>
              </span>
            </button>
          )}
          <button className="m-item" onClick={() => useStore.setState({ transfersOpen: true })}>
            <span className="m-item-icon" data-tint><Icon name="forward" size={22} /></span>
            <span className="m-item-text"><strong>Transfers</strong><span>Copies and moves in progress</span></span>
          </button>
          <button className="m-item" onClick={openSettings}>
            <span className="m-item-icon" data-tint><Icon name="settings" size={22} /></span>
            <span className="m-item-text"><strong>Settings</strong></span>
          </button>
        </div>
      </Section>
    </>
  );
}

/** Android: the card that asks for access to shared storage, until it is given. */
function AccessCard() {
  const os = useStore((s) => s.info?.os);
  const [granted, setGranted] = useState<boolean | null>(() => storageAccess());
  useEffect(() => {
    // Coming back from the system's permission page: look again, and reload what could not be read.
    const check = () => {
      const now = storageAccess();
      setGranted((was) => {
        if (was === false && now) {
          const s = useStore.getState();
          s.refreshDrives();
          s.bumpDirs(s.tabs.map((t) => t.location));
        }
        return now;
      });
    };
    window.addEventListener("fast-storage-access", check);
    document.addEventListener("visibilitychange", check);
    window.addEventListener("focus", check);
    return () => {
      window.removeEventListener("fast-storage-access", check);
      document.removeEventListener("visibilitychange", check);
      window.removeEventListener("focus", check);
    };
  }, []);
  if (os !== "android" || granted !== false) return null;
  return (
    <div className="m-access" role="alert">
      <strong>Allow access to your files</strong>
      <span>Android shows a file manager nothing until you allow it. Tap the button, then switch on "Allow access to manage all files" for this app.</span>
      <button className="primary" onClick={askStorageAccess}>Open the permission page</button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The viewer for files no other app can be asked to open (iOS)
// ---------------------------------------------------------------------------

const TEXT_LIMIT = 512 * 1024;
const MIME: Record<string, string> = { pdf: "application/pdf", svg: "image/svg+xml", mp4: "video/mp4", m4v: "video/mp4", mov: "video/quicktime", webm: "video/webm", mp3: "audio/mpeg", m4a: "audio/mp4", wav: "audio/wav", ogg: "audio/ogg", flac: "audio/flac", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", heic: "image/heic" };

type Shown = { url: string } | { text: string } | { error: string };

function Viewer() {
  const path = useStore((s) => s.viewing);
  const [shown, setShown] = useState<Shown | null>(null);
  const kind = path ? kindOf(path, false) : "";
  const pdf = !!path && /\.pdf$/i.test(path);
  const media = kind === "image" || kind === "video" || kind === "audio" || pdf;
  useEffect(() => {
    setShown(null);
    if (!path) return;
    let alive = true;
    let url = "";
    api
      .readForView(path)
      .then(async (bytes) => {
        if (!alive) return;
        const ext = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
        if (media) {
          url = URL.createObjectURL(new Blob([bytes], { type: MIME[ext] ?? "" }));
          return setShown({ url });
        }
        const head = new Uint8Array(bytes.slice(0, 4096));
        // A zero byte this early means it is not text.
        if (head.includes(0)) return setShown({ error: "" });
        const text = new TextDecoder().decode(bytes.slice(0, TEXT_LIMIT));
        setShown({ text: bytes.byteLength > TEXT_LIMIT ? `${text}\n\n… (the first 512 KB of ${formatSize(bytes.byteLength)})` : text });
      })
      .catch((e) => alive && setShown({ error: String(e) }));
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [path]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!path) return null;
  const close = () => useStore.setState({ viewing: null });
  const failed = (why?: string) => setShown({ error: why ?? "" });
  return (
    <div className="m-viewer" role="dialog" aria-label={baseName(path)}>
      <header>
        <button className="m-icon-button" aria-label="Close" onClick={close}><Icon name="close" size={20} /></button>
        <strong>{baseName(path)}</strong>
      </header>
      <div className="m-viewer-body" data-kind={kind}>
        {!shown ? (
          <div className="loading-bar" />
        ) : "error" in shown ? (
          <p className="m-empty">{shown.error || "This kind of file cannot be shown here, and this device gives the app no way to hand it to another app yet. Use Share from the file's menu, or open it from the Files app."}</p>
        ) : "text" in shown ? (
          <pre>{shown.text}</pre>
        ) : kind === "image" ? (
          <img src={shown.url} alt="" onError={() => failed()} />
        ) : kind === "video" ? (
          <video src={shown.url} controls playsInline onError={() => failed()} />
        ) : kind === "audio" ? (
          <audio src={shown.url} controls onError={() => failed()} />
        ) : (
          <iframe src={shown.url} title={baseName(path)} />
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The shell
// ---------------------------------------------------------------------------

/** Close whatever is open on top, innermost first. True when something was closed. */
function closeTopmost(): boolean {
  const s = useStore.getState();
  if (document.querySelector(".menu-backdrop")) {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    return true;
  }
  if (s.dialog) return s.dialog.resolve(null), true;
  if (s.viewing) return useStore.setState({ viewing: null }), true;
  if (s.properties) return useStore.setState({ properties: null }), true;
  if (s.shareFor) return useStore.setState({ shareFor: null }), true;
  if (s.batchRename) return useStore.setState({ batchRename: null }), true;
  if (s.openWith) return s.closeChooser(), true;
  if (s.connect) return s.closeConnect(), true;
  if (s.lockDialog) return useStore.setState({ lockDialog: null }), true;
  if (s.settingsOpen) return useStore.setState({ settingsOpen: false }), true;
  if (s.transfersOpen) return useStore.setState({ transfersOpen: false }), true;
  if (s.noticesOpen) return useStore.setState({ noticesOpen: false }), true;
  if (s.selecting) return useStore.setState({ selecting: false }), s.runCommand("selectNone"), true;
  return false;
}

/** One step back: up a folder, out of a search, or to the start page. False at the start page. */
function stepBack(roots: Root[]): boolean {
  const s = useStore.getState();
  const tab = s.tabs.find((t) => t.id === s.activeTab);
  if (!tab) return false;
  const loc = parseLocation(tab.location);
  if (loc.kind === "home" || loc.kind === "thispc") {
    if (loc.kind === "home" && adv<string>(s.settings, "ui.chrome") !== "samsung") return s.navigate(THIS_PC), true;
    return false;
  }
  if (loc.kind === "folder") {
    const up = phoneParent(loc.path, roots);
    s.navigate(up ?? THIS_PC);
    return true;
  }
  if (tab.historyIndex > 0) s.back();
  else s.navigate(THIS_PC);
  return true;
}

export function MobileApp({ location, overlays }: { location: string; overlays: ReactNode }) {
  const chrome = usePhoneChrome();
  const settings = useStore((s) => s.settings)!;
  const os = useStore((s) => s.info?.os);
  const clipboard = useStore((s) => s.clipboard);
  const selecting = useStore((s) => s.selecting);
  const sort = useStore((s) => s.sort);
  const folderView = useStore((s) => s.folderView);
  const pins = useStore((s) => s.pins);
  const theme = settings.appearance.theme;
  const roots = useRoots();
  const rootsRef = useRef(roots);
  rootsRef.current = roots;
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const { navigate, runCommand, updateSettings, setSort, setViewLevel, setClipboard, pasteInto, togglePin, openSettings } = useStore.getState();
  const loc = parseLocation(location);
  const atRoot = loc.kind === "home" || loc.kind === "thispc";
  const level = VIEW_LEVELS[Math.max(0, Math.min(VIEW_LEVELS.length - 1, viewLevelOf(settings, folderView)))];
  const asList = level === "details" || level === "list" || level === "small";

  // The system's own back button and gesture (Android): close what is on top, else go up, else leave.
  useEffect(() => {
    if (os !== "android") return;
    let off: (() => void) | undefined;
    let gone = false;
    void import("@tauri-apps/api/app")
      .then(async (app) => {
        const listener = await app.onBackButtonPress(() => {
          if (closeTopmost() || stepBack(rootsRef.current)) return;
          void app.exit(0).catch(() => {});
        });
        if (gone) void listener.unregister();
        else off = () => void listener.unregister();
      })
      .catch(() => {});
    return () => {
      gone = true;
      off?.();
    };
  }, [os]);

  // Android draws the app under the status and navigation bars; color those strips like the page.
  useEffect(() => {
    const t = setTimeout(() => {
      const bg = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#000000";
      const dark = getComputedStyle(document.documentElement).colorScheme.includes("dark");
      colorSystemBars(bg, !dark);
    }, 60);
    return () => clearTimeout(t);
  }, [theme, chrome]);

  useEffect(() => {
    setSearching(false);
    setQuery(loc.kind === "search" ? loc.query : "");
  }, [location]); // eslint-disable-line react-hooks/exhaustive-deps

  const title =
    loc.kind === "folder"
      ? isRemote(loc.path) && phoneParent(loc.path, roots) === null
        ? remoteLabel(loc.path)
        : phoneTitle(loc.path, roots)
      : loc.kind === "search"
        ? `“${loc.query}”`
        : loc.kind === "analyze"
          ? "Storage"
          : loc.kind === "home"
            ? "Recents"
            : chrome === "samsung"
              ? "My Files"
              : "Browse";
  const up = loc.kind === "folder" ? phoneParent(loc.path, roots) : null;
  const backLabel = loc.kind === "folder" ? (up ? phoneTitle(up, roots) : "Browse") : "Back";
  const scope = loc.kind === "folder" && !isArchiveLoc(loc.path) ? loc.path : loc.kind === "search" ? loc.scope : null;

  const submitSearch = () => {
    const q = query.trim();
    if (!q) return;
    useStore.getState().addSearch(q);
    navigate(searchLocation(q, scope));
  };

  const openMenu = (ev: React.MouseEvent) => {
    const r = ev.currentTarget.getBoundingClientRect();
    const common: MenuItem[] = [
      { id: "m-hidden", label: "Show hidden files", checked: settings.general.showHidden, separatorBefore: true, run: () => updateSettings((x) => void (x.general.showHidden = !x.general.showHidden)) },
      { id: "m-transfers", label: "Transfers", icon: "forward", run: () => useStore.setState({ transfersOpen: true }) },
      { id: "m-notices", label: "Recent actions", icon: "info", run: () => useStore.setState({ noticesOpen: true, unreadNotices: 0 }) },
      { id: "m-settings", label: "Settings", icon: "settings", run: openSettings },
    ];
    const folder: MenuItem[] =
      loc.kind === "folder"
        ? [
            { id: "m-select", label: "Select", icon: "check", run: () => runCommand("select") },
            { id: "m-new-folder", label: "New folder", icon: "folderOpen", run: () => runCommand("newFolder") },
            { id: "m-new-file", label: "New file", icon: "newItem", run: () => runCommand("newFile") },
            { id: "m-paste", label: "Paste", icon: "paste", disabled: !clipboard, run: () => runCommand("paste") },
            { id: "m-view-list", label: "List", icon: "list", checked: asList, separatorBefore: true, run: () => setViewLevel(0) },
            { id: "m-view-grid", label: chrome === "apple" ? "Icons" : "Grid", icon: "grid", checked: !asList, run: () => setViewLevel(3) },
            {
              id: "m-sort",
              label: `Sort by: ${SORTS.find(([k]) => k === sort.key)?.[1] ?? "Name"}`,
              icon: "sort",
              children: [
                ...SORTS.map(([k, label]) => ({ id: `m-sort-${k}`, label, checked: sort.key === k, run: () => setSort({ key: k, asc: sort.key === k ? sort.asc : k === "name" || k === "type" }) })),
                { id: "m-sort-asc", label: "Ascending", checked: sort.asc, separatorBefore: true, run: () => setSort({ ...sort, asc: true }) },
                { id: "m-sort-desc", label: "Descending", checked: !sort.asc, run: () => setSort({ ...sort, asc: false }) },
              ],
            },
            { id: "m-refresh", label: "Refresh", icon: "refresh", run: () => runCommand("refresh") },
            ...(isRemote(loc.path) || isArchiveLoc(loc.path)
              ? []
              : [
                  { id: "m-pin", label: pins.some((p) => p.path === loc.path) ? "Remove from favorites" : "Add to favorites", icon: "star", run: () => togglePin(loc.path) },
                  { id: "m-analyze", label: "Analyze storage here", icon: "chart", run: () => navigate(analyzeLocation(loc.path)) },
                ]),
            { id: "m-more", label: "More for this folder…", icon: "more", run: () => runCommand("menu") },
          ]
        : [];
    setMenu({ x: r.right, y: r.bottom, items: [...folder, ...common] });
  };

  const searchField = (placeholder: string, auto: boolean) => (
    <form
      className="m-search"
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        (document.activeElement as HTMLElement | null)?.blur();
        submitSearch();
      }}
    >
      <Icon name="search" size={18} />
      <input type="search" enterKeyHint="search" autoFocus={auto} value={query} placeholder={placeholder} aria-label={placeholder} onChange={(e) => setQuery(e.target.value)} />
      {query && (
        <button type="button" className="m-icon-button" aria-label="Clear" onClick={() => setQuery("")}>
          <Icon name="close" size={16} />
        </button>
      )}
    </form>
  );
  const menuButton = (
    <button className="m-icon-button m-menu-button" aria-label="More" aria-haspopup="menu" onClick={openMenu}>
      <Icon name="more" size={20} />
    </button>
  );
  const crumbsRow = loc.kind === "folder" && (
    <nav className="m-crumbs" aria-label="Path">
      {phoneCrumbs(loc.path, roots).map((c, i, all) => (
        <button key={c.path} disabled={i === all.length - 1} onClick={() => navigate(c.path)}>
          {i > 0 && <Icon name="chevron" size={10} />}
          <span>{c.label}</span>
        </button>
      ))}
    </nav>
  );

  return (
    <div className="app m-app" data-phone={chrome} data-os={os} data-root={atRoot || undefined} data-selecting={selecting || undefined}>
      {atRoot ? (
        <header className="m-top m-top-root">
          {chrome === "google" ? (
            <div className="m-bar">{searchField("Search files", false)}{menuButton}</div>
          ) : (
            <>
              <div className="m-bar">
                <span className="grow" />
                {chrome === "samsung" && (
                  <button className="m-icon-button" aria-label="Search" onClick={() => setSearching((v) => !v)}><Icon name="search" size={20} /></button>
                )}
                {menuButton}
              </div>
              <h1 className="m-large-title">{title}</h1>
              {(chrome === "apple" || searching) && <div className="m-bar">{searchField("Search", chrome === "samsung")}</div>}
            </>
          )}
        </header>
      ) : (
        <header className="m-top">
          <div className="m-bar">
            <button className="m-back" aria-label={`Back to ${backLabel}`} onClick={() => void stepBack(roots)}>
              <Icon name={chrome === "google" ? "back" : "chevron"} size={chrome === "google" ? 20 : 18} />
              {chrome === "apple" && <span>{backLabel}</span>}
            </button>
            {searching ? (
              searchField(loc.kind === "folder" ? `Search in ${title}` : "Search", true)
            ) : chrome === "samsung" && crumbsRow ? (
              crumbsRow
            ) : (
              <h1 className="m-title">{title}</h1>
            )}
            {(loc.kind === "folder" || loc.kind === "search") && (
              <button className="m-icon-button" aria-label={searching ? "Close search" : "Search"} onClick={() => setSearching((v) => !v)}>
                <Icon name={searching ? "close" : "search"} size={20} />
              </button>
            )}
            {loc.kind === "folder" && chrome === "google" && (
              <button className="m-icon-button" aria-label={asList ? "Show as a grid" : "Show as a list"} onClick={() => setViewLevel(asList ? 3 : 0)}>
                <Icon name={asList ? "grid" : "list"} size={20} />
              </button>
            )}
            {menuButton}
          </div>
          {chrome !== "samsung" && crumbsRow}
        </header>
      )}

      <main className="m-content" key={location}>
        {atRoot ? (
          <div className="m-page">
            <AccessCard />
            {chrome === "samsung" ? (
              <>
                <Section title="Recent files"><Recents limit={6} /></Section>
                <Browse chrome={chrome} />
              </>
            ) : loc.kind === "home" ? (
              <Section title={chrome === "google" ? "Recent" : ""}><Recents limit={60} /></Section>
            ) : (
              <Browse chrome={chrome} />
            )}
          </div>
        ) : (
          <>
            {loc.kind === "folder" && <AccessCard />}
            {loc.kind === "search" && <SearchView query={loc.query} scope={loc.scope} />}
            {loc.kind === "analyze" && <AnalyzerView path={loc.path} />}
            {loc.kind === "folder" && <FileView path={loc.path} />}
          </>
        )}
      </main>

      {clipboard && !selecting && (
        <div className="m-paste" role="toolbar" aria-label="Paste">
          <button className="m-text-button" onClick={() => setClipboard(null)}>Cancel</button>
          <span className="grow">
            {clipboard.paths.length === 1 ? baseName(clipboard.paths[0]) : `${clipboard.paths.length} items`}
          </span>
          <button className="primary" disabled={loc.kind !== "folder"} onClick={() => loc.kind === "folder" && void pasteInto(loc.path).then(() => setClipboard(null))}>
            {clipboard.op === "move" ? "Move here" : "Copy here"}
          </button>
        </div>
      )}

      {chrome !== "samsung" && !selecting && (
        <nav className="m-nav" aria-label="Pages">
          {(
            [
              [HOME, "Recents", "clock"],
              [THIS_PC, "Browse", "folderOpen"],
            ] as const
          ).map(([to, label, icon]) => {
            const on = to === HOME ? loc.kind === "home" : loc.kind !== "home";
            return (
              <button key={to} data-on={on || undefined} aria-current={on ? "page" : undefined} onClick={() => navigate(to)}>
                <span className="m-nav-icon"><Icon name={icon} size={22} /></span>
                <span>{label}</span>
              </button>
            );
          })}
        </nav>
      )}

      {overlays}
      <Viewer />
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
