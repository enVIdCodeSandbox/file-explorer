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
          <option value="">Choose a collection…</option>
          {(packs.list?.packs ?? []).map((p) => (
            <option key={p.id} value={p.id}>
              {p.label} — {p.about} ({p.installed ? "installed, " : ""}{sizeNote(p)})
            </option>
          ))}
        </select>
      </div>
      {!id && <p className="muted icon-packs-note">Extra collections are not part of the app, so it stays small. Pick one to see a preview from the internet, then install it if you like. {EXPLAIN}</p>}
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
