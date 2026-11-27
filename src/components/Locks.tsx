// In-app passcodes for folders and drives.
//
// What this is: a privacy screen inside FAST File Explorer. A locked folder or
// drive cannot be opened, listed, previewed, searched or copied from here until
// the passcode is entered. What it is not: encryption. Other programs, the
// operating system and other file managers can still reach the files, which is
// what "without affecting access elsewhere" means. For files that must be
// unreadable to others, use disk or file encryption.

import { useEffect, useRef, useState } from "react";
import { api, type LocksState, type SshCredentialStatus } from "../api";
import { adv } from "../settings/advanced";
import { ask, useStore } from "../store";
import { Icon } from "./Icons";

const MIN_PASSCODE = 4;
const MIN_MAIN = 6;

/** Shown in place of a folder that is locked. */
export function LockedCard({ path, onOpened }: { path: string; onOpened: () => void }) {
  const [text, setText] = useState("");
  const [wrong, setWrong] = useState(false);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), [path]);
  const submit = async () => {
    if (!text || busy) return;
    setBusy(true);
    const ok = await api.lockUnlock(path, text).catch(() => false);
    setBusy(false);
    if (ok) {
      setText("");
      setWrong(false);
      useStore.getState().bumpDirs([path]);
      onOpened();
    } else {
      setWrong(true);
      input.current?.select();
    }
  };
  return (
    <div className="empty-state lock-card">
      <Icon name="lock" size={28} />
      <strong>{path} is locked</strong>
      <span>Enter its passcode to open it. This only locks the folder inside this app.</span>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <input
          ref={input}
          className="text-input"
          type="password"
          autoComplete="off"
          aria-label="Passcode"
          placeholder="Passcode"
          value={text}
          data-wrong={wrong || undefined}
          onChange={(e) => {
            setText(e.target.value);
            setWrong(false);
          }}
        />
        <button className="primary" type="submit" disabled={!text || busy}>
          Unlock
        </button>
      </form>
      {wrong && <span className="error-text">That passcode is not right.</span>}
    </div>
  );
}

/** "Lock with passcode…": set a passcode for a folder or drive. */
export function LockDialog() {
  const path = useStore((s) => s.lockDialog);
  const close = () => useStore.setState({ lockDialog: null });
  const [state, setState] = useState<LocksState | null>(null);
  const [pass, setPass] = useState("");
  const [again, setAgain] = useState("");
  const [main, setMain] = useState("");
  const [mainAgain, setMainAgain] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const first = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!path) return;
    setPass("");
    setAgain("");
    setMain("");
    setMainAgain("");
    setError("");
    void api.locksList().then(setState).catch(() => setState({ hasMain: false, locks: [] }));
    setTimeout(() => first.current?.focus(), 30);
  }, [path]);

  if (!path) return null;
  const needMain = state ? !state.hasMain : false;
  const submit = async () => {
    if (pass.length < MIN_PASSCODE) return setError(`Use at least ${MIN_PASSCODE} characters for the passcode.`);
    if (pass !== again) return setError("The two passcodes are different.");
    if (needMain) {
      if (main.length < MIN_MAIN) return setError(`Use at least ${MIN_MAIN} characters for the main password.`);
      if (main !== mainAgain) return setError("The two main passwords are different.");
      if (main === pass) return setError("The main password should differ from the passcode.");
    }
    setBusy(true);
    try {
      await api.lockAdd(path, pass, needMain ? main : undefined);
      useStore.getState().showToast("Locked. It stays unlocked for now; it locks again when the time set in Settings has passed.");
      useStore.getState().bumpDirs([path]);
      close();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <form
        className="dialog lock-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Lock with passcode"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          e.stopPropagation();
        }}
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <h3>Lock with passcode</h3>
        <p className="path">{path}</p>
        <p className="muted">
          Locks this {path.length <= 3 || /^\/(Volumes|mnt|media|run\/media)\/[^/]+$/.test(path) ? "drive" : "folder"} inside this app only: it cannot be opened, listed, searched, previewed or copied from here without the passcode. Other programs can still reach the files, so this is a privacy screen, not encryption.
        </p>
        <label>
          Passcode
          <input ref={first} className="text-input" type="password" autoComplete="off" value={pass} onChange={(e) => setPass(e.target.value)} />
        </label>
        <label>
          Type it again
          <input className="text-input" type="password" autoComplete="off" value={again} onChange={(e) => setAgain(e.target.value)} />
        </label>
        {needMain && (
          <>
            <p className="muted">
              Choose a main password too. It is needed to remove a lock later (Settings &gt; Privacy and locks), so a forgotten passcode never leaves a folder stuck, and nobody can remove your locks without it.
            </p>
            <label>
              Main password
              <input className="text-input" type="password" autoComplete="off" value={main} onChange={(e) => setMain(e.target.value)} />
            </label>
            <label>
              Main password again
              <input className="text-input" type="password" autoComplete="off" value={mainAgain} onChange={(e) => setMainAgain(e.target.value)} />
            </label>
          </>
        )}
        {error && <p className="error-text">{error}</p>}
        <div className="dialog-buttons">
          <button type="button" className="secondary" onClick={close}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={busy || !state}>
            Lock
          </button>
        </div>
      </form>
    </div>
  );
}

/** Settings > Privacy and locks: every lock, the main password, and how long unlocks last. */
export function LocksPanel() {
  const settings = useStore((s) => s.settings);
  const [state, setState] = useState<LocksState | null>(null);
  const [secret, setSecret] = useState<Record<string, string>>({});
  const [old, setOld] = useState("");
  const [fresh, setFresh] = useState("");
  const [message, setMessage] = useState("");
  const { showToast } = useStore.getState();
  const reload = () => void api.locksList().then(setState).catch((e) => showToast(String(e)));
  useEffect(reload, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (!state) return <p className="muted">Loading…</p>;
  const relock = adv<number>(settings, "locks.relockMin");
  const remove = async (id: string) => {
    try {
      setState(await api.lockRemove(id, secret[id] ?? ""));
      setSecret((s) => ({ ...s, [id]: "" }));
      useStore.getState().bumpDirs([]);
      showToast("Lock removed.");
    } catch (e) {
      showToast(String(e));
    }
  };
  const changeMain = async () => {
    try {
      setState(await api.lockSetMain(old, fresh));
      setOld("");
      setFresh("");
      setMessage("Main password saved.");
    } catch (e) {
      setMessage(String(e));
    }
  };
  return (
    <div className="locks-panel">
      <p className="muted">
        A lock hides a folder or drive inside this app until its passcode is entered. It is a privacy screen, not encryption: other programs and other file managers can still reach the files. To add one, right-click a folder or a drive and choose “Lock with passcode…” (it is the last item in the menu). Unlocks last {relock > 0 ? `${relock} minutes` : "until the app closes"}; change that in Advanced.
      </p>
      {state.locks.length === 0 ? (
        <p>No folders or drives are locked.</p>
      ) : (
        <table className="storage-table">
          <thead>
            <tr>
              <th>Locked</th>
              <th>Now</th>
              <th>{state.hasMain ? "Main password" : "Its passcode"}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {state.locks.map((l) => (
              <tr key={l.id}>
                <th scope="row">
                  <span className="path">{l.path}</span>
                </th>
                <td>{l.open ? "Unlocked" : "Locked"}</td>
                <td>
                  <input
                    className="text-input"
                    type="password"
                    autoComplete="off"
                    aria-label={`Password to remove the lock on ${l.path}`}
                    value={secret[l.id] ?? ""}
                    onChange={(e) => setSecret((s) => ({ ...s, [l.id]: e.target.value }))}
                  />
                </td>
                <td>
                  <button className="secondary" disabled={!secret[l.id]} onClick={() => void remove(l.id)}>
                    Remove lock
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="button-row">
        <button
          className="secondary"
          disabled={!state.locks.some((l) => l.open)}
          onClick={() => void api.locksRelock().then((s) => { setState(s); showToast("Everything is locked again."); })}
        >
          Lock everything now
        </button>
      </div>
      <h4>Main password</h4>
      <p className="muted">
        {state.hasMain ? "Required to remove any lock. Enter the current one to change it." : "None yet. It is created with your first lock, and without it a lock is removed with its own passcode."}
      </p>
      <div className="button-row">
        {state.hasMain && (
          <input className="text-input" type="password" autoComplete="off" placeholder="Current main password" value={old} onChange={(e) => setOld(e.target.value)} />
        )}
        <input className="text-input" type="password" autoComplete="off" placeholder="New main password" value={fresh} onChange={(e) => setFresh(e.target.value)} />
        <button className="secondary" disabled={!fresh || (state.hasMain && !old)} onClick={() => void changeMain()}>
          {state.hasMain ? "Change" : "Set"}
        </button>
      </div>
      {message && <p className="muted">{message}</p>}
    </div>
  );
}

/** Settings > Privacy and locks: the optional password asked for before anything is deleted for good. */
export function DeleteGuardPanel() {
  const settings = useStore((s) => s.settings);
  const [has, setHas] = useState<boolean | null>(null);
  const [old, setOld] = useState("");
  const [fresh, setFresh] = useState("");
  const [again, setAgain] = useState("");
  const [message, setMessage] = useState("");
  useEffect(() => {
    void api.deleteGuardStatus().then(setHas).catch(() => setHas(false));
  }, []);
  if (has === null) return <p className="muted">Loading…</p>;
  const on = adv<boolean>(settings, "files.deletePassword");
  const toggle = (value: boolean) => useStore.getState().updateSettings((s) => void (s.advanced = { ...s.advanced, "files.deletePassword": value }));
  const save = async (remove: boolean) => {
    if (!remove && fresh !== again) {
      setMessage("The two new passwords are not the same.");
      return;
    }
    try {
      const now = await api.deleteGuardSet(old, remove ? "" : fresh);
      setHas(now);
      setOld("");
      setFresh("");
      setAgain("");
      if (remove) toggle(false);
      setMessage(remove ? "Password removed, and the option turned off." : "Password saved.");
    } catch (e) {
      setMessage(String(e));
    }
  };
  return (
    <div className="locks-panel">
      <p className="muted">
        When this is on, deleting for good (Shift+Delete, Delete permanently, emptying the Recycle Bin or Trash, deleting from a server, a phone or an archive) first asks for this password. Moving things to the Recycle Bin or Trash never asks. It protects against accidents; it does not stop other programs on this computer. If you forget it, the option can be turned off in Advanced, but changing it needs the old password, so write it down.
      </p>
      <label className="check-row">
        <input type="checkbox" checked={on} disabled={!has} onChange={(e) => toggle(e.target.checked)} />
        <span>Ask for a password before deleting for good</span>
      </label>
      {!has && <p className="muted">Set a password below first, then this can be turned on.</p>}
      <div className="button-row">
        {has && <input className="text-input" type="password" autoComplete="off" placeholder="Current password" value={old} onChange={(e) => setOld(e.target.value)} />}
        <input className="text-input" type="password" autoComplete="off" placeholder="New password (4 or more characters)" value={fresh} onChange={(e) => setFresh(e.target.value)} />
        <input className="text-input" type="password" autoComplete="off" placeholder="New password again" value={again} onChange={(e) => setAgain(e.target.value)} />
        <button className="secondary" disabled={!fresh || (has && !old)} onClick={() => void save(false)}>
          {has ? "Change" : "Set"}
        </button>
        {has && (
          <button className="secondary" disabled={!old} onClick={() => void save(true)}>
            Remove password
          </button>
        )}
      </div>
      {message && <p className="muted">{message}</p>}
    </div>
  );
}

/** Explicit, high-visibility consent prompt before allowing SSH keys/logins to be saved across sessions. */
export async function confirmSaveSshLogins(): Promise<boolean> {
  const r = await ask({
    title: "Not recommended: Save SSH keys and logins?",
    message:
      "IT IS HIGHLY RECOMMENDED NOT TO ENABLE THIS.\n\n" +
      "Saving your SSH key file location, username, and server lets FAST File Explorer reconnect automatically after closing and reopening the app. Anyone who can use this account or read your app data folder could reach those SSH servers without signing in again.\n\n" +
      "Only turn this on if you understand and accept the risk. You can disable it anytime in Settings > Privacy and locks.",
    buttons: [
      { id: "cancel", label: "Keep disabled (recommended)", primary: true },
      { id: "enable", label: "I understand the risk — enable", danger: true },
    ],
  });
  return r?.button === "enable";
}

/** Explicit consent prompt before saving SSH key passphrases or login passwords between sessions. */
export async function confirmSaveSshPassphrase(): Promise<boolean> {
  const r = await ask({
    title: "Not recommended: Save SSH key passphrases?",
    message:
      "IT IS HIGHLY RECOMMENDED NOT TO SAVE PASSPHRASES.\n\n" +
      "Saving your key passphrase or login password stores it on this computer between sessions. Anyone able to read your app data folder could unlock your SSH key without knowing your passphrase.\n\n" +
      "Only consent if you accept this risk. You can disable it anytime in Settings > Privacy and locks.",
    buttons: [
      { id: "cancel", label: "Do not save passphrase (recommended)", primary: true },
      { id: "enable", label: "I consent — save passphrase", danger: true },
    ],
  });
  return r?.button === "enable";
}

/** Toggle saving SSH key paths/logins between sessions, prompting with a warning on enable and clearing saved credentials on disable. */
export async function toggleRememberSshLogins(enable: boolean): Promise<boolean> {
  const st = useStore.getState();
  if (enable) {
    const ok = await confirmSaveSshLogins();
    if (!ok) return false;
    await st.updateSettings((s) => {
      s.rememberSshLogins = true;
    });
    return true;
  }
  await st.updateSettings((s) => {
    s.rememberSshLogins = false;
    s.rememberSshPassphrase = false;
  });
  const servers = await api.sshCredentialsClear(true).catch(() => null);
  if (servers) st.setServers(servers);
  return true;
}

/** Toggle saving SSH key passphrases/passwords between sessions, clearing stored passphrases on disable. */
export async function toggleRememberSshPassphrase(enable: boolean): Promise<boolean> {
  const st = useStore.getState();
  if (enable) {
    if (!st.settings?.rememberSshLogins) {
      const okLogin = await toggleRememberSshLogins(true);
      if (!okLogin) return false;
    }
    const ok = await confirmSaveSshPassphrase();
    if (!ok) return false;
    await st.updateSettings((s) => {
      s.rememberSshPassphrase = true;
    });
    return true;
  }
  await st.updateSettings((s) => {
    s.rememberSshPassphrase = false;
  });
  await api.sshCredentialsClear(false).catch(() => null);
  return true;
}

/** Settings > Privacy and locks: opt-in persistence for SSH key paths, usernames, servers, and consented passphrases. */
export function SshLoginsPanel() {
  const rememberLogins = useStore((s) => !!s.settings?.rememberSshLogins);
  const rememberPassphrase = useStore((s) => !!s.settings?.rememberSshPassphrase);
  const [status, setStatus] = useState<SshCredentialStatus>({ count: 0, withPassphrase: 0 });
  const refresh = () => void api.sshCredentialsStatus().then(setStatus).catch(() => {});
  useEffect(refresh, [rememberLogins, rememberPassphrase]);

  const clearAll = async () => {
    const servers = await api.sshCredentialsClear(true).catch(() => null);
    if (servers) useStore.getState().setServers(servers);
    refresh();
    useStore.getState().showToast("Saved SSH logins and passphrases cleared.");
  };

  return (
    <div className="locks-panel">
      <div className="update-failed" role="note">
        <strong>Highly recommended not to turn this on</strong>
        <p>
          Saving SSH key file locations, usernames, servers, or passphrases lets FAST File Explorer reconnect automatically after you close and reopen the app. Anyone who can access this user account or app data folder could connect to those servers without asking. Keep these options off unless you understand the risk.
        </p>
      </div>
      <label className="check-row">
        <input
          type="checkbox"
          checked={rememberLogins}
          onChange={(e) => void toggleRememberSshLogins(e.target.checked).then(refresh)}
        />
        <span>Save SSH key file location, username, and server between sessions</span>
      </label>
      <label className="check-row" style={{ marginLeft: 20 }}>
        <input
          type="checkbox"
          checked={rememberPassphrase}
          disabled={!rememberLogins}
          onChange={(e) => void toggleRememberSshPassphrase(e.target.checked).then(refresh)}
        />
        <span>Also save the key passphrase / password between sessions (requires explicit consent)</span>
      </label>
      <div className="button-row">
        <span className="muted">
          {status.count === 0
            ? "No SSH logins or passphrases are saved on disk."
            : `${status.count} saved SSH login${status.count === 1 ? "" : "s"}${status.withPassphrase > 0 ? ` (${status.withPassphrase} with passphrase saved)` : " (passphrase not saved)"}.`}
        </span>
        {status.count > 0 && (
          <button className="secondary" onClick={() => void clearAll()}>
            Clear saved credentials
          </button>
        )}
      </div>
    </div>
  );
}
