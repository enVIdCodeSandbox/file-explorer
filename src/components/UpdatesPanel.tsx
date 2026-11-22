// Settings > Updates: which build this is, whether a newer one is out, and the buttons to get it.

import { useEffect, useState } from "react";
import { api, on, type UpdateInfo } from "../api";
import { useStore } from "../store";
import { formatDate, formatSize } from "../util/format";

/** "0.3.0 (release)" or "f6e5d4c (build)". */
export const latestLabel = (l: UpdateInfo["latest"]) => (!l ? "" : l.source === "release" ? `${l.version ?? l.sha.slice(0, 7)} (release)` : `${l.sha.slice(0, 7)} (build)`);

export function UpdatesPanel() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [busy, setBusy] = useState<"check" | "download" | "install" | null>(null);
  const [error, setError] = useState("");
  const enabled = useStore((s) => s.settings?.updates.enabled);

  useEffect(() => {
    void api.updateStatus().then(setInfo).catch(() => {});
    const offs = [on("update-ready", setInfo), on("update-available", setInfo), on("update-failed", setInfo)];
    return () => offs.forEach((o) => void Promise.resolve(o).then((f) => f?.()));
  }, []);

  const run = async (kind: "check" | "download" | "install", job: () => Promise<unknown>) => {
    setBusy(kind);
    setError("");
    try {
      const r = await job();
      if (kind === "check") setInfo(r as UpdateInfo);
      else setInfo(await api.updateStatus());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  if (!info) return <p className="muted">…</p>;
  const latest = info.latest;
  const when = info.lastCheck ? formatDate(info.lastCheck * 1000) : "never";
  const newer = info.status === "available" || info.status === "unknown";
  return (
    <div className="updates-panel">
      <dl className="details-list">
        <dt>This build</dt>
        <dd>
          {info.version}
          {info.sha ? <span className="muted"> · {info.sha.slice(0, 7)}</span> : <span className="muted"> · made by hand (no build number)</span>}
        </dd>
        {latest && (
          <>
            <dt>Newest build</dt>
            <dd>
              {latestLabel(latest)} <span className="muted">· {formatDate(Date.parse(latest.createdAt))} · {formatSize(latest.size)}</span>
            </dd>
          </>
        )}
        <dt>Last checked</dt>
        <dd>{when}</dd>
      </dl>
      <p className={info.status === "error" ? "error-text" : "muted"}>
        {info.status === "available" ? "A newer build is available." : info.message || (enabled === false ? "Updates are turned off." : "")}
      </p>
      {info.failed && (
        <div className="update-failed" role="alert">
          <strong>The last update did not install.</strong>
          <p>{info.failed.note}</p>
          <p className="muted">It is not downloaded again by itself. You can run the installer yourself and watch where it installs, or try the automatic update once more.</p>
          <div className="row-buttons">
            <button className="primary" disabled={busy != null} onClick={() => void run("install", api.updateInstallManual)}>
              Run the installer myself
            </button>
            <button className="secondary" disabled={busy != null} onClick={() => void run("check", async () => { await api.updateForgetFailure(); return api.updateStatus(); })}>
              Try the automatic update again
            </button>
          </div>
        </div>
      )}
      {error && <p className="error-text">{error}</p>}
      <div className="row-buttons">
        <button className="secondary" disabled={busy != null || enabled === false} onClick={() => void run("check", api.updateCheck)}>
          {busy === "check" ? "Checking…" : "Check now"}
        </button>
        {newer && !info.ready && !info.failed && (
          <button className="primary" disabled={busy != null} onClick={() => void run("download", api.updateDownload)}>
            {busy === "download" ? "Starting…" : info.status === "unknown" ? "Install the newest build" : "Update now"}
          </button>
        )}
        {info.ready && (
          <>
            <button className="primary" disabled={busy != null} onClick={() => void run("install", api.updateInstall)}>
              Restart to finish updating
            </button>
            <button className="secondary" disabled={busy != null} onClick={() => void run("check", async () => { await api.updateDiscard(); return api.updateStatus(); })}>
              Discard the download
            </button>
          </>
        )}
      </div>
      {newer && !info.ready && <p className="muted">The download appears with your other transfers. When it is done you choose whether to restart now or later.</p>}
    </div>
  );
}
