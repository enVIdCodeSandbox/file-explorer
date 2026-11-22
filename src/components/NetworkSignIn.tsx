// Sign in again to network drives that are already connected: tick the drives (all of them, or just a few), type the new
// user name and password once, and each is connected again at the same letter and address, without making it again.

import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { driveName } from "../util/drives";
import { explainNetError } from "./driveActions";

type Outcome = { mount: string; ok: boolean; error: string | null };

export function NetworkSignIn() {
  const request = useStore((s) => s.signIn);
  const drives = useStore((s) => s.drives);
  const appearance = useStore((s) => s.settings?.appearance);
  const network = useMemo(() => drives.filter((d) => d.media === "network"), [drives]);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const [working, setWorking] = useState(false);
  const [results, setResults] = useState<Outcome[] | null>(null);

  useEffect(() => {
    if (!request) return;
    const pick = request.preselect.length ? request.preselect : network.map((d) => d.mount);
    setTicked(new Set(pick));
    setUser("");
    setPassword("");
    setResults(null);
    setWorking(false);
    // only when it opens
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request]);

  if (!request || !appearance) return null;
  const close = () => useStore.setState({ signIn: null });
  const name = (mount: string) => {
    const d = network.find((x) => x.mount === mount);
    return d ? driveName(d, appearance) : mount;
  };
  const toggle = (mount: string) =>
    setTicked((prev) => {
      const next = new Set(prev);
      next.has(mount) ? next.delete(mount) : next.add(mount);
      return next;
    });
  const canGo = !working && ticked.size > 0 && user.trim() !== "";

  const go = async () => {
    if (!canGo) return;
    setWorking(true);
    try {
      const done = await api.networkRelogin([...ticked], user.trim(), password);
      setResults(done);
      useStore.getState().refreshDrives();
      const failed = done.filter((r) => !r.ok).length;
      if (!failed) {
        useStore.getState().showToast(done.length === 1 ? "Signed in again." : `Signed in again to ${done.length} network drives.`);
        close();
      }
    } catch (e) {
      useStore.getState().showToast(String(e));
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={() => !working && close()}>
      <div
        className="dialog signin-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Sign in again to network drives"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape" && !working) close();
          if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) void go();
          e.stopPropagation();
        }}
      >
        <h3>Sign in again to network drives</h3>
        <p className="dialog-message">
          Pick the drives and enter the new user name and password. Each one is connected again at the same letter, with nothing to set up again. Windows allows one sign-in per server, so tick every drive on a server together.
        </p>
        {network.length === 0 ? (
          <p className="muted">There are no network drives connected.</p>
        ) : (
          <>
            <div className="signin-tools">
              <button className="link-button" onClick={() => setTicked(new Set(network.map((d) => d.mount)))}>All</button>
              <button className="link-button" onClick={() => setTicked(new Set())}>None</button>
              <span className="muted">{ticked.size} of {network.length} ticked</span>
            </div>
            <div className="signin-list" role="group" aria-label="Network drives">
              {network.map((d) => {
                const r = results?.find((x) => x.mount === d.mount);
                return (
                  <label key={d.mount} className="signin-row" data-result={r ? (r.ok ? "ok" : "failed") : undefined}>
                    <input type="checkbox" checked={ticked.has(d.mount)} disabled={working} onChange={() => toggle(d.mount)} />
                    <span className="grow file-name">{name(d.mount)}</span>
                    {r && <span className="signin-status">{r.ok ? "Signed in" : "Not signed in"}</span>}
                    {r && !r.ok && <span className="signin-why muted">{explainNetError(r.error ?? "")}</span>}
                  </label>
                );
              })}
            </div>
            <label className="dialog-select" data-stacked>
              <span className="muted">User name (DOMAIN\name for a domain)</span>
              <input className="text-input" value={user} autoFocus autoComplete="off" disabled={working} onChange={(e) => setUser(e.target.value)} />
            </label>
            <label className="dialog-select" data-stacked>
              <span className="muted">Password</span>
              <input className="text-input" type="password" value={password} autoComplete="off" disabled={working} onChange={(e) => setPassword(e.target.value)} />
            </label>
          </>
        )}
        <div className="dialog-buttons">
          <button className="primary" disabled={!canGo} onClick={() => void go()}>
            {working ? "Signing in…" : results ? "Try again" : "Sign in"}
          </button>
          <button className="secondary" disabled={working} onClick={close}>
            {results ? "Close" : "Cancel"}
          </button>
        </div>
      </div>
    </div>
  );
}
