// Installing extra icon collections. They are not part of the app: a collection is looked at over the network first
// and installed on request, and the space it takes is shown beside it, with a way to remove it again.

import { useCallback, useEffect, useMemo, useState } from "react";
import { api, on } from "../api";
import { safeSvg } from "../util/customIcons";
import { formatSize } from "../util/format";
import { matchesQuery, packIconLabel, parsePack, packFraction, packProgressText, type PackIcon, type PackList, type PackProgress, type PackState } from "../util/iconPacks";

/** Draw one already-checked icon. */
export function PackSvg({ svg }: { svg: string }) {
  return <span className="pack-svg" aria-hidden dangerouslySetInnerHTML={{ __html: svg }} />;
}

/**
 * What is installed, and the actions that change it. Several collections can download at the same time (each is small),
 * so every one has its own progress; removing waits until none is downloading.
 */
export function usePacks() {
  const [list, setList] = useState<PackList | null>(null);
  const [downloading, setDownloading] = useState<ReadonlySet<string>>(new Set());
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");
  const [progress, setProgress] = useState<Record<string, PackProgress>>({});
  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    on("icon-pack-progress", (p) => alive && setProgress((all) => ({ ...all, [p.id]: p }))).then((u) => (alive ? (off = u) : u()), () => {});
    return () => {
      alive = false;
      off?.();
    };
  }, []);
  const reload = useCallback(() => api.iconPacksList().then(setList, () => setList((l) => l ?? { packs: [], disk: 0 })), []);
  useEffect(() => {
    let alive = true;
    api.iconPacksList().then((l) => alive && setList(l), () => alive && setList({ packs: [], disk: 0 }));
    return () => {
      alive = false;
    };
  }, []);
  const install = useCallback(
    async (id: string) => {
      setDownloading((d) => new Set(d).add(id));
      setProgress((all) => ({ ...all, [id]: { id, phase: "connecting", done: 0, total: 0 } }));
      setError("");
      try {
        await api.iconPackInstall(id);
        return true;
      } catch (e) {
        setError(String(e).replace(/^Error: /, ""));
        return false;
      } finally {
        setDownloading((d) => {
          const n = new Set(d);
          n.delete(id);
          return n;
        });
        setProgress(({ [id]: _gone, ...rest }) => rest);
        // Other downloads may have finished meanwhile, so the list is read again instead of trusting this answer.
        void reload();
      }
    },
    [reload],
  );
  const exclusive = useCallback(
    async (job: () => Promise<PackList>) => {
      setRemoving(true);
      setError("");
      try {
        setList(await job());
        return true;
      } catch (e) {
        setError(String(e).replace(/^Error: /, ""));
        return false;
      } finally {
        setRemoving(false);
      }
    },
    [],
  );
  return {
    list,
    downloading,
    /** Something is being removed, or anything is downloading: removing is then not offered. */
    locked: removing || downloading.size > 0,
    removing,
    progress,
    error,
    setError,
    install,
    remove: (id: string) => exclusive(() => api.iconPackRemove(id)),
    clear: () => exclusive(() => api.iconPacksClear()),
  };
}
export type Packs = ReturnType<typeof usePacks>;

/** What a download shows: a bar that fills, and words for the stage it is in. */
export function DownloadBar({ packs, id }: { packs: Packs; id: string }) {
  if (!packs.downloading.has(id)) return null;
  const p = packs.progress[id] ?? null;
  const fraction = p ? packFraction(p) : null;
  return (
    <div className="pack-progress" role="progressbar" aria-label="Downloading" aria-valuemin={0} aria-valuemax={100} aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}>
      <div className="pack-progress-bar" data-indeterminate={fraction === null ? "" : undefined} style={fraction === null ? undefined : { width: `${fraction * 100}%` }} />
      <span className="muted pack-progress-text">{p ? packProgressText(p) : "Starting…"}</span>
    </div>
  );
}

/** Label for an install button while it works. */
export const installLabel = (packs: Packs, id: string, idle: string) => {
  if (!packs.downloading.has(id)) return idle;
  const p = packs.progress[id] ?? null;
  return p && p.phase === "downloading" ? `Downloading ${Math.round(packFraction(p) * 100)}%` : p?.phase === "saving" ? "Saving…" : "Connecting…";
};

export const sizeNote = (p: PackState) => (p.installed ? `uses ${formatSize(p.disk)}` : `about ${formatSize(p.download)} to install`);

const EXPLAIN = "Once installed they work with no network, and icons you have already chosen keep working even if a collection is removed later.";

/** Installed collections, read from the computer once and kept for the run (dropped again when one is removed). */
const readCache = new Map<string, Promise<PackIcon[]>>();
const readInstalled = (id: string) => {
  let r = readCache.get(id);
  if (!r) {
    r = api.iconPackRead(id).then(parsePack, () => {
      readCache.delete(id);
      return [] as PackIcon[];
    });
    readCache.set(id, r);
  }
  return r;
};

/** How many icons a collection shows under its heading before "Show all". */
const PER_SECTION = 36;

interface Section {
  id: string;
  label: string;
  about: string;
  installed: boolean;
  total: number;
  icons: { name: string; svg: string }[];
}

/**
 * What was typed, looked for in every collection. Installed ones are searched on this computer, the others over the
 * network (a few matches each, with how many there are in all). Each collection has a heading, so you can see where an
 * icon is from, and one that is not installed can be installed from its heading.
 */
export function AllCollections({ query, packs, onPick }: { query: string; packs: Packs; onPick: (svg: string) => void }) {
  const list = packs.list?.packs ?? [];
  const installedKey = list.filter((p) => p.installed).map((p) => p.id).join(",");
  const openKey = list.filter((p) => !p.installed).map((p) => p.id).join(",");
  const [local, setLocal] = useState<Record<string, { name: string; svg: string }[]>>({});
  const [remote, setRemote] = useState<{ hits: { id: string; total: number; pack: string }[]; failed: string | null } | null>(null);
  const [searching, setSearching] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  useEffect(() => setExpanded(new Set()), [query]);

  // Installed collections: on this computer, at once.
  useEffect(() => {
    let alive = true;
    const ids = installedKey ? installedKey.split(",") : [];
    for (const k of [...readCache.keys()]) if (!ids.includes(k)) readCache.delete(k);
    Promise.all(ids.map(async (id) => [id, await readInstalled(id)] as const)).then((all) => {
      if (!alive) return;
      const out: Record<string, { name: string; svg: string }[]> = {};
      for (const [id, icons] of all) {
        const hit: { name: string; svg: string }[] = [];
        for (const i of icons) if (matchesQuery(i.name, query)) hit.push(i);
        if (hit.length) out[id] = hit;
      }
      setLocal(out);
    });
    return () => {
      alive = false;
    };
  }, [installedKey, query]);

  // The rest: over the network, after a short pause in typing.
  useEffect(() => {
    const ids = openKey ? openKey.split(",") : [];
    if (!ids.length) {
      setRemote({ hits: [], failed: null });
      setSearching(false);
      return;
    }
    let alive = true;
    setSearching(true);
    const timer = setTimeout(() => {
      api.iconPacksSearch(ids, query, PER_SECTION).then(
        (r) => alive && (setRemote(r), setSearching(false)),
        (e) => alive && (setRemote({ hits: [], failed: String(e).replace(/^Error: /, "") }), setSearching(false)),
      );
    }, 400);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [openKey, query]);

  const sections = useMemo(() => {
    const out: Section[] = [];
    const ready = (icons: { name: string; svg: string }[]) => {
      const clean: { name: string; svg: string }[] = [];
      for (const i of icons) {
        const svg = safeSvg(i.svg);
        if (svg) clean.push({ name: i.name, svg });
      }
      return clean;
    };
    for (const p of list) {
      if (p.installed && local[p.id]) {
        const all = local[p.id];
        out.push({ id: p.id, label: p.label, about: p.about, installed: true, total: all.length, icons: ready(expanded.has(p.id) ? all : all.slice(0, PER_SECTION)) });
      }
    }
    out.sort((a, b) => b.total - a.total || a.label.localeCompare(b.label));
    const net: Section[] = [];
    for (const h of remote?.hits ?? []) {
      const p = list.find((x) => x.id === h.id);
      if (p && !p.installed) net.push({ id: p.id, label: p.label, about: p.about, installed: false, total: h.total, icons: ready(parsePack(h.pack)) });
    }
    return [...out, ...net];
  }, [list, local, remote, expanded]);

  const matches = sections.reduce((n, s) => n + s.total, 0);
  return (
    <div className="icon-all" aria-label="Icons from every collection">
      <div className="muted icon-packs-count">
        {searching ? "Searching every collection…" : sections.length ? `${matches}${remote?.hits.length ? "+" : ""} icons in ${sections.length} ${sections.length === 1 ? "collection" : "collections"}` : "No icon in any collection matches."}
        {remote?.failed && !searching ? ` Collections that are not installed could not be searched (${remote.failed.replace(/^Could not reach the internet to get the icons /, "no network ").replace(/[()]/g, "")}).` : ""}
      </div>
      {sections.map((s) => (
        <section key={s.id} className="icon-section" aria-label={s.label}>
          <h4 className="icon-section-head">
            <span>
              {s.label} <span className="muted">· {s.total} {s.total === 1 ? "match" : "matches"} · {s.installed ? "installed" : "preview from the internet"}</span>
            </span>
            {!s.installed && (
              <button className="secondary" disabled={packs.downloading.has(s.id) || packs.removing} onClick={() => void packs.install(s.id)}>
                {installLabel(packs, s.id, `Install (${formatSize(list.find((p) => p.id === s.id)?.download ?? 0)})`)}
              </button>
            )}
          </h4>
          {!s.installed && <DownloadBar packs={packs} id={s.id} />}
          <div className="icon-grid icon-grid-pack" role="listbox" aria-label={`${s.label} icons`}>
            {s.icons.map((i) => (
              <button key={i.name} className="icon-cell" title={`${packIconLabel(i.name)} (${s.label})`} aria-label={`${packIconLabel(i.name)} from ${s.label}`} onClick={() => onPick(i.svg)}>
                <PackSvg svg={i.svg} />
              </button>
            ))}
            {s.installed && s.total > s.icons.length && (
              <button className="secondary icon-more" onClick={() => setExpanded((e) => new Set(e).add(s.id))}>
                Show all {s.total}
              </button>
            )}
            {!s.installed && s.total > s.icons.length && <span className="muted icon-more">and {s.total - s.icons.length} more. Install to browse them all.</span>}
          </div>
        </section>
      ))}
    </div>
  );
}

/** Inside the icon picker: choose a collection, see its icons (a preview from the internet until it is installed) and install it. */
export function PackPicker({ filter, packs, onPick }: { filter: string; packs: Packs; onPick: (svg: string) => void }) {
  const [id, setId] = useState("");
  const state = packs.list?.packs.find((p) => p.id === id);
  const [icons, setIcons] = useState<PackIcon[] | null>(null);
  const [total, setTotal] = useState(0);
  const [problem, setProblem] = useState("");
  const [limit, setLimit] = useState(240);
  const installed = !!state?.installed;
  const wanted = filter.trim();

  // An installed collection is read from the computer once; one that is not is previewed over the network.
  useEffect(() => {
    setLimit(240);
    setProblem("");
    if (!id) {
      setIcons(null);
      return;
    }
    let alive = true;
    if (installed) {
      setIcons(null);
      api.iconPackRead(id).then(
        (json) => alive && (setIcons(parsePack(json)), setTotal(0)),
        () => alive && setProblem("That collection could not be opened. Remove it and install it again."),
      );
      return () => {
        alive = false;
      };
    }
    setIcons(null);
    const timer = setTimeout(() => {
      api.iconPackPreview(id, wanted, 60).then(
        (r) => alive && (setIcons(parsePack(r.pack)), setTotal(r.total)),
        (e) => alive && setProblem(String(e).replace(/^Error: /, "")),
      );
    }, wanted ? 350 : 0);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
    // The preview follows what is typed; an installed collection is filtered on screen instead.
  }, [id, installed, installed ? "" : wanted]);

  const shown = useMemo(() => {
    if (!icons) return [];
    const matching = installed && wanted ? icons.filter((i) => matchesQuery(i.name, wanted)) : icons;
    const out: { name: string; svg: string }[] = [];
    for (const i of matching) {
      if (out.length >= limit) break;
      const svg = safeSvg(i.svg);
      if (svg) out.push({ name: i.name, svg });
    }
    return out;
  }, [icons, installed, wanted, limit]);
  const matchCount = installed && icons ? (wanted ? icons.filter((i) => matchesQuery(i.name, wanted)).length : icons.length) : total;

  return (
    <div className="icon-packs">
      <div className="icon-packs-bar">
        <label htmlFor="icon-pack">More icons</label>
        <select id="icon-pack" className="text-input" value={id} onChange={(e) => setId(e.target.value)}>
          <option value="">Search every collection</option>
          {(packs.list?.packs ?? []).map((p) => (
            <option key={p.id} value={p.id}>
              {p.label} — {p.about} ({p.installed ? "installed, " : ""}{sizeNote(p)})
            </option>
          ))}
        </select>
      </div>
      {!id && wanted.length >= 2 && packs.list && <AllCollections query={wanted} packs={packs} onPick={onPick} />}
      {!id && wanted.length < 2 && <p className="muted icon-packs-note">Type in "Find an icon" to look in every collection at once, with a heading for each one. Or pick a collection here to browse it. Extra collections are not part of the app, so it stays small: the ones you have not installed show as a preview from the internet, and you can install them if you like. {EXPLAIN}</p>}
      {state && !installed && (
        <div className="icon-packs-install">
          <span className="muted">
            A preview from the internet. Installing downloads about <b>{formatSize(state.download)}</b> once and keeps it in this app's cache folder. {EXPLAIN} Remove it any time here or under Settings → Storage.
          </span>
          <button className="primary" disabled={packs.downloading.has(state.id) || packs.removing} onClick={() => void packs.install(state.id)}>
            {installLabel(packs, state.id, `Install (${formatSize(state.download)})`)}
          </button>
        </div>
      )}
      {state && !installed && <DownloadBar packs={packs} id={state.id} />}
      {state && installed && (
        <div className="icon-packs-install">
          <span className="muted">Installed: uses <b>{formatSize(state.disk)}</b> on this computer. Works with no network.</span>
          <button className="secondary" disabled={packs.locked} onClick={() => void packs.remove(state.id).then(() => setId(state.id))}>
            Remove (frees {formatSize(state.disk)})
          </button>
        </div>
      )}
      {(problem || packs.error) && <div className="dialog-error" role="alert">{problem || packs.error}</div>}
      {id && !problem && (
        <>
          <div className="muted icon-packs-count">
            {icons ? (installed ? `${matchCount} icons${wanted ? " match" : ""}` : total > shown.length ? `Showing ${shown.length} of ${total} icons${wanted ? " that match" : ""}. Install to browse them all.` : `${total} icons${wanted ? " match" : ""}`) : "Loading…"}
          </div>
          <div className="icon-grid icon-grid-pack" role="listbox" aria-label={`${state?.label ?? ""} icons`}>
            {shown.map((i) => (
              <button key={i.name} className="icon-cell" title={packIconLabel(i.name)} aria-label={packIconLabel(i.name)} onClick={() => onPick(i.svg)}>
                <PackSvg svg={i.svg} />
              </button>
            ))}
            {icons && shown.length === 0 && <div className="side-empty">No icon matches.</div>}
            {installed && matchCount > limit && <button className="secondary icon-more" onClick={() => setLimit((n) => n + 360)}>Show more ({matchCount - limit} left)</button>}
          </div>
        </>
      )}
      {packs.list && packs.list.disk > 0 && (
        <div className="icon-packs-foot">
          <span className="muted">Installed collections use {formatSize(packs.list.disk)}.</span>
          <button className="secondary" disabled={packs.locked} onClick={() => void packs.clear().then(() => setId((x) => x))}>
            Clear icon cache
          </button>
        </div>
      )}
    </div>
  );
}

/** In Settings: every collection with its size, to install or remove, and one button to clear them all. */
export function PackManager() {
  const packs = usePacks();
  const list = packs.list;
  return (
    <div className="pack-manager">
      <p className="muted">
        Extra icon collections are not part of the app. Install the ones you want (each is downloaded once); {EXPLAIN} They are also listed under Settings → Storage.
      </p>
      {packs.error && <div className="dialog-error" role="alert">{packs.error}</div>}
      <div className="icons-list">
        {(list?.packs ?? []).map((p) => (
          <div key={p.id} className="icons-row" data-changed={p.installed ? "" : undefined}>
            <span className="icons-name">
              {p.label} <span className="muted">— {p.about}</span>
            </span>
            <span className="muted pack-size">{sizeNote(p)}</span>
            {p.installed ? (
              <button className="secondary" disabled={packs.locked} onClick={() => void packs.remove(p.id)}>Remove</button>
            ) : (
              <button className="secondary" disabled={packs.downloading.has(p.id) || packs.removing} onClick={() => void packs.install(p.id)}>{installLabel(packs, p.id, "Install")}</button>
            )}
            {packs.downloading.has(p.id) && <div className="pack-row-progress"><DownloadBar packs={packs} id={p.id} /></div>}
          </div>
        ))}
      </div>
      <div className="icon-packs-foot">
        <span className="muted">{list && list.disk > 0 ? `Installed collections use ${formatSize(list.disk)}.` : "No collections installed, so nothing is using space."}</span>
        <button className="secondary" disabled={!list || list.disk === 0 || packs.locked} onClick={() => void packs.clear()}>Clear icon cache</button>
      </div>
    </div>
  );
}
