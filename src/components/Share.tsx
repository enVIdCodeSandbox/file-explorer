// Temporary download links: pick how long and how often, get a link to send.

import { useCallback, useEffect, useState } from "react";
import { api, type ShareInfo } from "../api";
import { useStore } from "../store";
import { formatSize } from "../util/format";
import { Icon } from "./Icons";

const DURATIONS: [number, string][] = [
  [10, "10 minutes"],
  [60, "1 hour"],
  [24 * 60, "1 day"],
  [7 * 24 * 60, "7 days"],
];
const LIMITS: [number, string][] = [
  [1, "1 download"],
  [5, "5 downloads"],
  [0, "No limit"],
];

const left = (at: number) => {
  const s = Math.max(0, at - Math.floor(Date.now() / 1000));
  if (s >= 86400) return `${Math.ceil(s / 86400)} d left`;
  if (s >= 3600) return `${Math.ceil(s / 3600)} h left`;
  return `${Math.max(1, Math.ceil(s / 60))} min left`;
};

export function ShareDialog() {
  const files = useStore((s) => s.shareFor);
  const showToast = useStore((s) => s.showToast);
  const [minutes, setMinutes] = useState(60);
  const [limit, setLimit] = useState(1);
  const [reach, setReach] = useState<"lan" | "me" | "internet">("lan");
  const [tunnel, setTunnel] = useState(false);
  const [busy, setBusy] = useState(false);
  const [active, setActive] = useState<ShareInfo[]>([]);

  useEffect(() => {
    if (files) void api.shareInternetAvailable().then(setTunnel).catch(() => setTunnel(false));
  }, [files]);

  const refresh = useCallback(() => void api.shareList().then(setActive).catch(() => {}), []);
  useEffect(() => {
    if (!files) return;
    refresh();
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [files, refresh]);

  if (!files) return null;
  const close = () => useStore.setState({ shareFor: null });

  const start = async () => {
    setBusy(true);
    try {
      for (const p of files) await api.shareStart(p, minutes, limit, reach === "lan", reach === "internet");
      refresh();
    } catch (e) {
      showToast(String(e));
    } finally {
      setBusy(false);
    }
  };
  const copy = (url: string) => void navigator.clipboard.writeText(url).then(() => showToast("Link copied."));
  const mail = (i: ShareInfo) => void api.openItem(`mailto:?subject=${encodeURIComponent(i.name)}&body=${encodeURIComponent(`Download ${i.name}: ${i.url}`)}`).catch(() => copy(i.url));

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div className="dialog share-dialog" role="dialog" aria-modal="true" aria-label="Temporary download link" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && close()}>
        <h3>Temporary download link</h3>
        <p className="dialog-message">
          {files.length === 1 ? files[0].split(/[\\/]/).pop() : `${files.length} items`} — anyone with the link can download it until it expires. Each file gets its own link.
        </p>
        <div className="share-options">
          <label>
            <span className="muted">Works for</span>
            <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
              {DURATIONS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </label>
          <label>
            <span className="muted">Limit</span>
            <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
              {LIMITS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </label>
          <label>
            <span className="muted">Reachable from</span>
            <select value={reach} onChange={(e) => setReach(e.target.value as "lan" | "me" | "internet")}>
              <option value="lan">Devices on this network</option>
              <option value="me">This computer only</option>
              <option value="internet">Anyone with the link (internet)</option>
            </select>
          </label>
        </div>
        {reach === "internet" ? (
          <p className="share-note muted">
            {tunnel
              ? "A temporary secure tunnel (Cloudflare quick tunnel, https) carries the download to your computer, so nothing on your router changes. The file is not uploaded anywhere, but it does pass through Cloudflare's network while it downloads. Anyone who gets the link can download the file until it expires, hits its limit, or you stop it, and only while this app is open."
              : "Sharing over the internet needs the free cloudflared program, which is not installed. Install it (developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads), make sure it is on your PATH, then reopen this dialog."}
          </p>
        ) : (
          <p className="share-note muted">
            The link only works while this app is open, and only on your network (not the internet). It is not encrypted, so share it with people you trust on a network you trust. Your firewall may ask for permission the first time.
          </p>
        )}
        {active.length > 0 && (
          <div className="share-list">
            {active.map((i) => (
              <div key={i.id} className="share-row">
                <div className="grow">
                  <div className="file-name">{i.name} <span className="muted">· {formatSize(i.size)}</span></div>
                  <input className="text-input share-url" readOnly value={i.url} onFocus={(e) => e.currentTarget.select()} />
                  <div className="muted">
                    {left(i.expiresAt)} · {i.downloads}{i.maxDownloads ? ` of ${i.maxDownloads}` : ""} downloaded · {i.internet ? "internet" : i.lan ? "network" : "this computer"}
                  </div>
                </div>
                <button className="icon-button" title="Copy link" aria-label="Copy link" onClick={() => copy(i.url)}><Icon name="copy" /></button>
                <button className="icon-button" title="Email the link" aria-label="Email the link" onClick={() => mail(i)}><Icon name="forward" /></button>
                <button className="icon-button danger" title="Stop sharing" aria-label="Stop sharing" onClick={() => void api.shareStop(i.id).then(refresh)}><Icon name="close" /></button>
              </div>
            ))}
          </div>
        )}
        <div className="dialog-buttons">
          <button className="secondary" onClick={close}>Close</button>
          <button className="primary" disabled={busy || (reach === "internet" && !tunnel)} onClick={() => void start()}>Create link</button>
        </div>
      </div>
    </div>
  );
}
