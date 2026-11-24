// macOS asks permission for Desktop, Documents, Downloads, Music, Photos, iCloud Drive and every external drive
// separately, unless the app is given Full Disk Access once. This file holds the three places that explain and ask
// for that one permission: a screen shown once (first run, or the first launch after an update that adds it), the
// buttons in Settings, and a note in Disk Usage. Nothing here shows on Windows or Linux.

import { useCallback, useEffect, useState } from "react";
import { api, type AccessStatus, type AskedFolder } from "../api";
import { useStore } from "../store";

/** The current permission state. It is looked at again when the window comes back, which is when the person returns
 * from System Settings, and, with `poll`, every couple of seconds (only while a screen that needs it is open). */
export function useAccess(poll = false): { status: AccessStatus | null; refresh: () => void } {
  const [status, setStatus] = useState<AccessStatus | null>(null);
  const refresh = useCallback(() => {
    api.accessStatus(true).then(setStatus).catch(() => setStatus(null));
  }, []);
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);
  useEffect(() => {
    if (!poll) return;
    // The switch in System Settings is noticed at once, with no need to press "check again". (Reading Safari's folder to
    // test it never shows a prompt.)
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, 2500);
    return () => window.clearInterval(timer);
  }, [poll, refresh]);
  return { status, refresh };
}

/** Change what is remembered about this screen. */
function remember(patch: Partial<{ asked: boolean; dismissed: boolean; verified: boolean; granted: string[]; grantedFor: string; resetFor: string }>) {
  const { settings, updateSettings, saveSettings } = useStore.getState();
  if (!settings) return;
  updateSettings((st) => {
    st.macAccess = { ...(st.macAccess ?? { off: [] }), asked: true, ...patch };
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
      const r = await api.accessAskFolders();
      setResult(r.folders);
      // Kept, so search, sizes and previews use the folders that were allowed (each covers everything inside it), and
      // so the app knows when macOS has forgotten them.
      remember({ granted: r.granted, grantedFor: r.token });
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

/** Whether access really works for the running app, as far as the app can tell (it tries to read a folder only
 * Full Disk Access opens), and what to do about it when it does not. */
function Steps({ status, refresh }: { status: AccessStatus; refresh: () => void }) {
  const unstable = status.identity === "perBuild";
  const forgot = useStore((s) => s.settings?.macAccess?.verified ?? false);
  const startOver = () =>
    void ask(async () => {
      await api.accessResetGrant(true);
      if (status.appPath) await api.accessRevealApp().catch(() => {});
      await api.accessOpenPane("fullDisk");
      refresh();
    });
  return status.fullAccess ? (
    <>
      <p className="dialog-message access-ok" role="status">
        <strong>Working.</strong> FAST File Explorer can open every folder without asking. (Checked just now by reading a folder that only Full Disk Access opens.)
      </p>
      <div className="dialog-buttons">
        <button className="secondary" onClick={() => void ask(api.restartApp)}>Restart the app</button>
      </div>
    </>
  ) : (
    <>
      <p className="access-warning" role="status">
        {forgot ? (
          <>
            <strong>macOS forgot the permission.</strong> It worked before, and an update made macOS treat this as a new app. Switch it on again once (remove the old entry first if it still shows as on); this screen goes away when it works.
          </>
        ) : (
          <>
            <strong>Not working yet.</strong> macOS is still refusing this copy of the app, so it will keep asking folder by folder. This screen goes away only when that has been checked and works.
          </>
        )}
      </p>
      {status.problem && (
        <p className="access-warning" role="alert">
          <strong>Move the app first. </strong>
          {status.problem}
        </p>
      )}
      <ol className="access-steps">
        <li>Press <strong>Open Full Disk Access</strong>. System Settings opens on that list.</li>
        <li>Switch on <strong>FAST File Explorer</strong>. If it is missing, press <strong>Show the app in Finder</strong> and drag it into the list.</li>
        <li>If it already shows as on but this screen still says <em>Not working yet</em>, the entry belongs to an earlier version of the app{unstable ? " (this build has no developer signature, so macOS ties every permission to one exact build)" : ""}. Press <strong>Remove the old entry and start over</strong> (or select it in System Settings, press <strong>−</strong> under the list, and drag the app in again with <strong>Show the app in Finder</strong>).</li>
        <li>Once switched on, click <strong>Restart the app</strong> (macOS only enables Full Disk Access after restarting the process).</li>
      </ol>
      <div className="dialog-buttons access-buttons">
        <button className="primary" onClick={() => void ask(() => api.accessOpenPane("fullDisk"))}>Open Full Disk Access</button>
        <button className="secondary" onClick={startOver}>Remove the old entry and start over</button>
        {status.appPath && <button className="secondary" onClick={() => void ask(api.accessRevealApp)}>Show the app in Finder</button>}
        <button className="secondary" onClick={refresh}>Check again</button>
        <button className="secondary" onClick={() => void ask(api.restartApp)}>Restart the app</button>
      </div>
      {unstable && (
        <p className="muted">
          Why this keeps coming back after updates: this copy of the app has no developer signature, so macOS treats each new version as a different app and forgets what it was allowed. The person who builds the app can fix that for good with a signing certificate (scripts/make-mac-signing-cert.sh).
        </p>
      )}
      <AskFolders refresh={refresh} />
    </>
  );
}

/** Shown at every launch until access has been verified, or until "Do not show this at startup" is chosen. Settings >
 * macOS permissions always has the same screen. */
export function MacAccessPrompt() {
  const settings = useStore((s) => s.settings);
  const [open, setOpen] = useState(false);
  const [leftAlone, setLeftAlone] = useState(false);
  const { status, refresh } = useAccess(open);
  const dismissed = settings?.macAccess?.dismissed ?? false;
  const verified = settings?.macAccess?.verified ?? false;

  // Remember that it worked, so that if macOS forgets it later (an update) the screen can say exactly that.
  useEffect(() => {
    if (settings && status?.applies && status.fullAccess && !verified) remember({ verified: true });
  }, [settings, status, verified]);

  // A build without a developer signature is a new app to macOS every time, so a Full Disk Access entry left by an
  // earlier build shows as on and does nothing. Clear it once per build, before the person is asked to switch it on.
  const resetFor = settings?.macAccess?.resetFor ?? "";
  useEffect(() => {
    if (!settings || !status?.applies || status.fullAccess || status.identity === "stable" || !status.token || resetFor === status.token) return;
    remember({ resetFor: status.token });
    void api.accessResetGrant().catch(() => {});
  }, [settings, status, resetFor]);

  useEffect(() => {
    if (open || leftAlone || !settings || dismissed || !status?.applies || status.fullAccess) return;
    setOpen(true);
    remember({ asked: true });
  }, [open, leftAlone, settings, dismissed, status]);

  // It closes itself once access works, after showing the result for a moment.
  useEffect(() => {
    if (!open || !status?.fullAccess) return;
    const t = window.setTimeout(() => setOpen(false), 2500);
    return () => window.clearTimeout(t);
  }, [open, status?.fullAccess]);

  if (!open || !status) return null;
  const later = () => {
    setOpen(false);
    setLeftAlone(true);
    if (!status.fullAccess) {
      useStore.getState().notify("error", "macOS will keep asking for each folder", "Full Disk Access is not working yet.", {
        important: true,
        seconds: 30,
        actions: [
          {
            label: "Set it up",
            run: () => {
              useStore.setState({ settingsSection: "macOS permissions" });
              useStore.getState().openSettings();
            },
          },
        ],
      });
    }
  };
  return (
    <div className="modal-backdrop" onMouseDown={later}>
      <div className="dialog access-dialog" role="dialog" aria-modal="true" aria-label="Allow access to all folders" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => (e.key === "Escape" && later(), e.stopPropagation())}>
        <h3>Let FAST File Explorer open your folders</h3>
        <p className="dialog-message">
          macOS asks permission folder by folder (Desktop, Documents, Downloads, Music, Photos, iCloud Drive, each external drive) every time something looks inside them. Full Disk Access is one switch that stops all of it. Settings, macOS permissions, has this screen whenever you want it.
        </p>
        <Steps status={status} refresh={refresh} />
        <div className="dialog-buttons">
          <button className="secondary" onClick={() => { remember({ dismissed: true }); setOpen(false); }}>Do not show this at startup</button>
          <button className="secondary" onClick={later}>{status.fullAccess ? "Close" : "Not now"}</button>
        </div>
      </div>
    </div>
  );
}

/** Settings > macOS permissions. */
export function MacAccessPanel() {
  const { status, refresh } = useAccess(true);
  const off = useStore((s) => s.settings?.macAccess?.off ?? []);
  const dismissed = useStore((s) => s.settings?.macAccess?.dismissed ?? false);
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
      <Steps status={status} refresh={refresh} />
      <label className="check-row">
        <input type="checkbox" checked={!dismissed} onChange={(e) => remember({ dismissed: !e.target.checked })} />
        <span>Show the permission screen when the app starts, until access works</span>
      </label>
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
