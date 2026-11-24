// macOS asks permission for Desktop, Documents, Downloads, Music, Photos, iCloud Drive and every external drive
// separately, unless the app is given Full Disk Access once. This file holds the three places that explain and ask
// for that one permission: a screen shown once (first run, or the first launch after an update that adds it), the
// buttons in Settings, and a note in Disk Usage. Nothing here shows on Windows or Linux.

import { useCallback, useEffect, useState } from "react";
import { api, type AccessStatus, type AskedFolder } from "../api";
import { useStore } from "../store";

/** The current permission state. It is looked at again when the window comes back, which is when the person returns
 * from System Settings. */
export function useAccess(): { status: AccessStatus | null; refresh: () => void } {
  const [status, setStatus] = useState<AccessStatus | null>(null);
  const refresh = useCallback(() => {
    api.accessStatus(true).then(setStatus).catch(() => setStatus(null));
  }, []);
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    // While access is not on, look again every couple of seconds: the switch in System Settings is noticed at once,
    // with no need to press "check again". (Reading Safari's folder to test it never shows a prompt.)
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, 2500);
    return () => {
      window.removeEventListener("focus", refresh);
      window.clearInterval(timer);
    };
  }, [refresh]);
  return { status, refresh };
}

/** Remember that the screen was shown, so it never comes back by itself. */
export function markAsked() {
  const { settings, updateSettings, saveSettings } = useStore.getState();
  if (!settings || settings.macAccess?.asked) return;
  updateSettings((s) => {
    s.macAccess = { ...(s.macAccess ?? { off: [] }), asked: true };
  });
  saveSettings();
}

const ask = (what: () => Promise<void>) => what().catch((e) => useStore.getState().showToast(String(e)));

/** The fallback to the one switch: ask about every usual folder right now, so macOS shows its questions together
 * (about seven, once; macOS remembers each answer). */
function AskFolders({ refresh }: { refresh: () => void }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AskedFolder[] | null>(null);
  const go = async () => {
    setBusy(true);
    try {
      setResult(await api.accessAskFolders());
    } catch (e) {
      useStore.getState().showToast(String(e));
    } finally {
      setBusy(false);
      refresh();
    }
  };
  const refused = result?.filter((r) => !r.granted) ?? [];
  return (
    <div className="access-fallback">
      <p className="muted">
        Would rather not change a system setting? The app can instead ask macOS about Desktop, Documents, Downloads, Music, Pictures, Movies and iCloud Drive right now, all together. That is up to seven questions, once, and macOS remembers the answers. External and network drives still ask the first time each is used, and search and Disk Usage stay out of the folders you refuse.
      </p>
      <div className="dialog-buttons access-buttons">
        <button className="secondary" disabled={busy} onClick={() => void go()}>{busy ? "Waiting for your answers…" : "Ask for the usual folders now"}</button>
      </div>
      {result && (
        <p className="muted" role="status">
          {refused.length ? `Not allowed: ${refused.map((r) => r.label).join(", ")}. You can change this in System Settings, Privacy & Security, Files and Folders.` : "All of them are allowed."}
        </p>
      )}
    </div>
  );
}

/** The steps and buttons, used by the screen and by Settings. */
function Steps({ status, refresh }: { status: AccessStatus; refresh: () => void }) {
  return status.fullAccess ? (
    <>
      <p className="dialog-message">
        Full access is on. FAST File Explorer can open every folder without asking again. If this was only just switched on, restart the app once so it takes effect everywhere.
      </p>
      <div className="dialog-buttons">
        <button className="primary" onClick={() => void ask(api.restartApp)}>Restart now</button>
      </div>
    </>
  ) : (
    <>
      {status.problem && (
        <p className="access-warning" role="alert">
          <strong>Move the app first. </strong>
          {status.problem}
        </p>
      )}
      <ol className="access-steps">
        <li>Press <strong>Allow access to all folders</strong>. System Settings opens on Full Disk Access.</li>
        <li>Switch on <strong>FAST File Explorer</strong> in the list. If it is not there, press <strong>Show the app in Finder</strong> and drag it into the list.</li>
        <li>Come back and press <strong>Restart</strong>. macOS then never asks again, for any folder or drive.</li>
      </ol>
      <div className="dialog-buttons access-buttons">
        <button className="primary" onClick={() => void ask(() => api.accessOpenPane("fullDisk"))}>Allow access to all folders</button>
        {status.appPath && <button className="secondary" onClick={() => void ask(api.accessRevealApp)}>Show the app in Finder</button>}
        <button className="secondary" onClick={refresh}>I did it: check again</button>
      </div>
      <details className="access-trouble">
        <summary>Switched on, and macOS still asks?</summary>
        <p className="muted">
          An entry in the list can look switched on and still belong to an earlier version of the app: each new build of an app without a developer signature is a different app to macOS. Press the button to remove the old entry, then switch the new one on and restart. The default file manager setting and Full Disk Access both need the app to stay in the Applications folder.
        </p>
        <div className="dialog-buttons access-buttons">
          <button
            className="secondary"
            onClick={() =>
              void ask(async () => {
                await api.accessResetGrant();
                if (status.appPath) await api.accessRevealApp().catch(() => {});
                await api.accessOpenPane("fullDisk");
                refresh();
              })
            }
          >
            Remove the old entry and start over
          </button>
        </div>
      </details>
      <AskFolders refresh={refresh} />
    </>
  );
}

/** Shown once, at the very first launch or the first launch after an update that adds it, and never again by itself. */
export function MacAccessPrompt() {
  const settings = useStore((s) => s.settings);
  const { status, refresh } = useAccess();
  const [open, setOpen] = useState(false);
  const asked = settings?.macAccess?.asked ?? true;

  useEffect(() => {
    if (open || !settings || asked || !status?.applies || status.fullAccess) return;
    setOpen(true);
    // Marked at once: even if the window is closed some other way, it does not come back.
    markAsked();
  }, [open, settings, asked, status]);

  if (!open || !status) return null;
  const close = () => setOpen(false);
  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div className="dialog access-dialog" role="dialog" aria-modal="true" aria-label="Allow access to all folders" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => (e.key === "Escape" && close(), e.stopPropagation())}>
        <h3>Let FAST File Explorer open your folders</h3>
        <p className="dialog-message">
          macOS asks for permission folder by folder (Desktop, Documents, Downloads, Music, Photos, iCloud Drive and each external drive) every time something looks inside them. One permission now stops all of that. You decide here, once; this screen does not come back, and you can change it any time under Settings, Permissions.
        </p>
        <Steps status={status} refresh={refresh} />
        <div className="dialog-buttons">
          <button className="secondary" onClick={close}>{status.fullAccess ? "Close" : "Not now"}</button>
        </div>
      </div>
    </div>
  );
}

/** Settings > Permissions. */
export function MacAccessPanel() {
  const { status, refresh } = useAccess();
  const off = useStore((s) => s.settings?.macAccess?.off ?? []);
  if (!status) return <p className="muted">Looking…</p>;
  if (!status.applies) return <p className="muted">This system has no per-folder permissions to set. Programs can open what your account can open.</p>;
  const toggle = (id: string, on: boolean) => {
    const { updateSettings, saveSettings } = useStore.getState();
    updateSettings((s) => {
      const base = s.macAccess ?? { asked: true, off: [] };
      s.macAccess = { ...base, off: on ? base.off.filter((x) => x !== id) : [...new Set([...base.off, id])] };
    });
    saveSettings();
  };
  return (
    <div className="access-panel">
      <p>
        <strong>{status.fullAccess ? "Full access is on." : "Full access is off."}</strong>{" "}
        <span className="muted">{status.fullAccess ? "The app opens every folder without asking." : "macOS asks about each kind of folder, and search, folder sizes and Disk Usage stay out of the protected ones."}</span>
      </p>
      <Steps status={status} refresh={refresh} />
      <p className="muted">Other permissions the app can ask for, and where to change any of them yourself in System Settings, Privacy & Security:</p>
      <div className="dialog-buttons access-buttons">
        <button className="secondary" onClick={() => void ask(() => api.accessOpenPane("files"))}>Files and Folders</button>
        <button className="secondary" onClick={() => void ask(() => api.accessOpenPane("photos"))}>Photos</button>
        <button className="secondary" onClick={() => void ask(() => api.accessOpenPane("media"))}>Media and Apple Music</button>
      </div>
      <p className="muted">Search the index, work out folder sizes and scan for Disk Usage in these places (needs full access):</p>
      <div className="access-list" role="group" aria-label="Places searched in the background">
        {status.categories.map((c) => (
          <label key={c.id} className="access-row">
            <input type="checkbox" checked={status.fullAccess && !off.includes(c.id)} disabled={!status.fullAccess} onChange={(e) => toggle(c.id, e.target.checked)} />
            <span className="grow">{c.label}</span>
            <span className="muted">{c.where}</span>
          </label>
        ))}
      </div>
    </div>
  );
}

/** A line at the top of Disk Usage when folders are being left out for want of permission. */
export function MacAccessNote() {
  const { status } = useAccess();
  if (!status?.applies || status.fullAccess) return null;
  return (
    <div className="access-note" role="note">
      <span className="grow">Disk Usage is leaving out Desktop, Documents, Downloads, Music, Pictures, Movies, iCloud Drive, other apps' data and external drives until you allow access. One permission covers all of them.</span>
      <button className="primary" onClick={() => void ask(() => api.accessOpenPane("fullDisk"))}>Allow access to all folders</button>
    </div>
  );
}

/** Under a folder or Disk Usage error: when macOS refused and the app has not been given full access, the one button
 * that fixes it. */
export function MacAccessHelp({ error }: { error: string }) {
  const { status } = useAccess();
  if (!status?.applies || status.fullAccess || !/operation not permitted|permission denied|os error (1|13)\b/i.test(error)) return null;
  return (
    <button className="primary" onClick={() => void ask(() => api.accessOpenPane("fullDisk"))}>
      Allow access to all folders
    </button>
  );
}
