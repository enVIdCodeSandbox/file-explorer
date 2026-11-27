import { useEffect, useRef, useState } from "react";
import { api, type SavedServer, type Server, type ServerProtocol } from "../api";
import { useStore } from "../store";
import { useTerminal } from "../terminal/state";
import { pickFile, sshFolder } from "../util/pickFile";
import { rawRemoteLabel, splitRemote } from "../util/path";

const PROTOCOLS: { id: ServerProtocol; label: string }[] = [
  { id: "sftp", label: "SFTP (SSH)" },
  { id: "ssh", label: "SSH" },
  { id: "ftp", label: "FTP" },
  { id: "dav", label: "WebDAV" },
];

const DEFAULT_PORT = (p: ServerProtocol, tls: boolean) => (p === "ftp" ? 21 : p === "sftp" || p === "ssh" ? 22 : tls ? 443 : 80);

interface Fields {
  protocol: ServerProtocol;
  host: string;
  port: string;
  user: string;
  password: string;
  keyFile: string;
  tls: boolean;
  path: string;
  remember: boolean;
  name: string;
}

const EMPTY: Fields = { protocol: "sftp", host: "", port: "", user: "", password: "", keyFile: "", tls: true, path: "", remember: true, name: "" };

/** Read `sftp://user@host:2222/folder` into form fields. */
export function fieldsFromUrl(url: string): Partial<Fields> {
  const m = /^([a-z]+):\/\//i.exec(url);
  if (!m) return {};
  const scheme = m[1].toLowerCase();
  const { origin, path } = splitRemote(url);
  const authority = origin.slice(m[0].length);
  const at = authority.lastIndexOf("@");
  const user = at >= 0 ? authority.slice(0, at) : "";
  const hostport = at >= 0 ? authority.slice(at + 1) : authority;
  const colon = hostport.lastIndexOf(":");
  const hasPort = colon > 0 && /^\d+$/.test(hostport.slice(colon + 1));
  return {
    protocol: scheme === "ftp" ? "ftp" : scheme === "sftp" ? "sftp" : scheme === "ssh" ? "ssh" : "dav",
    tls: scheme === "davs",
    user,
    host: hasPort ? hostport.slice(0, colon) : hostport,
    port: hasPort ? hostport.slice(colon + 1) : "",
    path: path === "/" ? "" : path,
  };
}

function fieldsFromSaved(s: SavedServer): Partial<Fields> {
  return { protocol: s.protocol, host: s.host, port: s.port === DEFAULT_PORT(s.protocol, s.tls) ? "" : String(s.port), user: s.user, tls: s.tls, keyFile: s.keyFile ?? "", path: s.path, name: s.name };
}

/** Log in to an FTP, SFTP or WebDAV server and open it as a folder. */
export function ConnectServer() {
  const request = useStore((s) => s.connect);
  const picking = useStore((s) => !!s.picker);
  const home = useStore((s) => s.info?.home);
  const [f, setF] = useState<Fields>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** A server key that has to be confirmed before logging in. */
  const [confirm, setConfirm] = useState<{ fingerprint: string; changed: boolean } | null>(null);
  const hostRef = useRef<HTMLInputElement>(null);
  const passRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!request) return;
    const base = request.saved ? fieldsFromSaved(request.saved) : request.url ? fieldsFromUrl(request.url) : {};
    setF({ ...EMPTY, ...base, remember: !request.url || !!request.saved });
    setBusy(false);
    setError(request.message ?? null);
    setConfirm(null);
    setTimeout(() => (base.host ? passRef.current : hostRef.current)?.focus(), 0);
  }, [request]);

  // While a file is being chosen the form steps aside (keeping what was typed).
  if (!request || picking) return null;

  const set = <K extends keyof Fields>(key: K, value: Fields[K]) => setF((x) => ({ ...x, [key]: value }));
  const close = () => useStore.getState().closeConnect();
  const port = f.port.trim() ? Number(f.port) : DEFAULT_PORT(f.protocol, f.tls);
  const valid = f.host.trim().length > 0 && Number.isInteger(port) && port > 0 && port < 65536;
  const plain = f.protocol === "ftp" || (f.protocol === "dav" && !f.tls);

  const connect = async (trust?: string) => {
    if (busy || !valid) return;
    setBusy(true);
    setError(null);
    const usesSshKey = f.protocol === "sftp" || f.protocol === "ssh";
    const server: Server = { protocol: f.protocol, host: f.host.trim(), port, user: f.user.trim(), password: f.password, keyFile: usesSshKey && f.keyFile.trim() ? f.keyFile.trim() : null, tls: f.protocol === "dav" && f.tls, trust: trust ?? null };
    try {
      const url = await api.remoteConnect(server, f.path.trim() || undefined);
      if (f.remember) {
        const entry: SavedServer = { name: f.name.trim() || rawRemoteLabel(url), protocol: server.protocol, host: server.host, port: server.port, user: server.user, tls: server.tls, keyFile: server.keyFile, path: f.path.trim() };
        useStore.getState().setServers(await api.serversSave(entry));
      }
      close();
      useStore.getState().navigate(url);
      if (server.protocol === "ssh") void useTerminal.getState().openForSsh(url);
    } catch (e) {
      const text = String(e instanceof Error ? e.message : e);
      if (text.startsWith("untrusted-host:")) setConfirm({ fingerprint: text.slice("untrusted-host:".length), changed: false });
      else if (text.startsWith("changed-host:")) setConfirm({ fingerprint: text.slice("changed-host:".length), changed: true });
      else setError(text);
      setBusy(false);
    }
  };

  if (confirm) {
    return (
      <div className="modal-backdrop" onMouseDown={close}>
        <div className="dialog connect-server" role="dialog" aria-modal="true" aria-label="Confirm the server" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => (e.key === "Escape" && close(), e.stopPropagation())}>
          <h3>{confirm.changed ? "This server's identity has changed" : `Trust ${f.host.trim()}?`}</h3>
          <p className="dialog-message">
            {confirm.changed
              ? "This server presented a different key than the one you trusted before. That can happen after a reinstall, but it can also mean someone is intercepting the connection."
              : "This is the first time you connect to this server. Check that the fingerprint matches the one your server's owner gave you."}
          </p>
          <p className="connect-fingerprint"><code>{confirm.fingerprint}</code></p>
          <div className="dialog-buttons">
            <button className={confirm.changed ? "danger" : "primary"} onClick={() => void connect(confirm.fingerprint)} disabled={busy} autoFocus>
              {confirm.changed ? "Trust the new key" : "Trust and connect"}
            </button>
            <button onClick={close}>Cancel</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div
        className="dialog connect-server"
        role="dialog"
        aria-modal="true"
        aria-label="Connect to server"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) void connect();
          e.stopPropagation();
        }}
      >
        <h3>Connect to server</h3>
        <label className="batch-field">
          <span>Address (paste a whole address to fill in the rest)</span>
          <input
            className="text-input"
            placeholder="sftp://me@files.example.com/photos or ssh://me@host"
            spellCheck={false}
            autoCapitalize="none"
            autoCorrect="off"
            inputMode="url"
            onChange={(e) => /^(s?ftp|ssh|davs?):\/\//i.test(e.target.value.trim()) && setF((x) => ({ ...x, ...fieldsFromUrl(e.target.value.trim()) }))}
          />
        </label>
        {error && <p className="connect-error" role="alert">{error}</p>}

        <div className="batch-row">
          <label className="batch-field narrow" style={{ width: 150 }}>
            <span>Type</span>
            <select value={f.protocol} onChange={(e) => set("protocol", e.target.value as ServerProtocol)}>
              {PROTOCOLS.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
            </select>
          </label>
          <label className="batch-field">
            <span>Server</span>
            <input ref={hostRef} className="text-input" value={f.host} placeholder="files.example.com" spellCheck={false} autoCapitalize="none" autoCorrect="off" inputMode="url" onChange={(e) => set("host", e.target.value)} />
          </label>
          <label className="batch-field narrow port">
            <span>Port</span>
            <input className="text-input" value={f.port} placeholder={String(DEFAULT_PORT(f.protocol, f.tls))} inputMode="numeric" onChange={(e) => set("port", e.target.value.replace(/\D/g, ""))} />
          </label>
        </div>

        <div className="batch-row">
          <label className="batch-field">
            <span>User name</span>
            <input className="text-input" value={f.user} placeholder={f.protocol === "ftp" ? "leave empty to log in anonymously" : ""} spellCheck={false} autoCapitalize="none" autoCorrect="off" autoComplete="off" onChange={(e) => set("user", e.target.value)} />
          </label>
          <label className="batch-field">
            <span>{(f.protocol === "sftp" || f.protocol === "ssh") && f.keyFile ? "Key passphrase" : "Password"}</span>
            <input ref={passRef} className="text-input" type="password" value={f.password} autoComplete="off" onChange={(e) => set("password", e.target.value)} />
          </label>
        </div>

        {(f.protocol === "sftp" || f.protocol === "ssh") && (
          <label className="batch-field">
            <span>Private key file (optional)</span>
            <div className="key-row">
              <input className="text-input" value={f.keyFile} placeholder="~/.ssh/id_ed25519" spellCheck={false} onChange={(e) => set("keyFile", e.target.value)} />
              <button className="secondary"
                type="button"
                title="Hide this form, browse to the key file in the window, and come back"
                onClick={() => void pickFile("Choose your private key file", sshFolder(home)).then((p) => p && set("keyFile", p))}
              >
                Browse…
              </button>
            </div>
          </label>
        )}
        {f.protocol === "dav" && (
          <label className="check-row">
            <input type="checkbox" checked={f.tls} onChange={(e) => set("tls", e.target.checked)} /> Use a secure connection (HTTPS)
          </label>
        )}
        <label className="batch-field">
          <span>Folder (optional)</span>
          <input className="text-input" value={f.path} placeholder={f.protocol === "dav" ? "/remote.php/dav/files/me" : "/home/me"} spellCheck={false} onChange={(e) => set("path", e.target.value)} />
        </label>

        {plain && (
          <p className="connect-warning" role="note">
            {f.protocol === "ftp" ? "FTP is not encrypted: your password and files can be read by others on the network. Use SFTP when the server offers it." : "Without HTTPS your password and files can be read by others on the network."}
          </p>
        )}

        <div className="batch-row" style={{ alignItems: "end" }}>
          <label className="check-row" style={{ marginTop: 14 }}>
            <input type="checkbox" checked={f.remember} onChange={(e) => set("remember", e.target.checked)} /> Keep this address in the sidebar
          </label>
          {f.remember && (
            <label className="batch-field">
              <span>Name</span>
              <input className="text-input" value={f.name} placeholder={f.host.trim() || "My server"} onChange={(e) => set("name", e.target.value)} />
            </label>
          )}
        </div>
        <p className="batch-hint muted">The password is only used for this session. It is never saved.</p>

        <div className="dialog-buttons">
          <button className="primary" onClick={() => void connect()} disabled={!valid || busy}>{busy ? "Connecting…" : "Connect"}</button>
          <button onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
