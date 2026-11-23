// Light read-only Permissions window opened from the right-click menu.

import { useEffect, useState } from "react";
import { api, type FilePermissions, type WindowsAce } from "../api";
import { useStore } from "../store";
import { maskAddresses } from "../util/path";
import { FileIcon, Icon } from "./Icons";

function formatSummaryText(p: FilePermissions): string {
  const lines = [`Name: ${p.name}`, `Path: ${p.path}`];
  if (p.owner) lines.push(`Owner: ${p.owner}`);
  if (p.group && p.group !== p.owner && !p.group.endsWith("\\None")) lines.push(`Group: ${p.group}`);
  if (p.readonly) lines.push("Attribute: Read-only");
  if (p.unix) {
    lines.push(`Mode: ${p.unix.modeText} (${p.unix.octal})`);
  }
  if (p.windows) {
    lines.push(`Inheritance: ${p.windows.inherited ? "Enabled" : "Disabled"}`);
    for (const e of p.windows.entries) {
      const origin = e.inherited ? "inherited" : "explicit";
      const scope = e.appliesTo ? ` · ${e.appliesTo}` : "";
      lines.push(`- ${e.account}: ${e.kind.toUpperCase()} ${e.level} (${origin}${scope})`);
    }
  }
  if (p.note) lines.push(`Note: ${p.note}`);
  return lines.join("\n");
}

function AceRow({ ace, index }: { ace: WindowsAce; index: number }) {
  const [expanded, setExpanded] = useState(() => ace.level === "Special");
  const hasRights = ace.rights.length > 0;

  return (
    <div className="perm-ace" data-kind={ace.kind} data-expanded={expanded || undefined}>
      <button
        type="button"
        className="perm-ace-row"
        aria-expanded={hasRights ? expanded : undefined}
        onClick={() => hasRights && setExpanded((v) => !v)}
        disabled={!hasRights}
        title={hasRights ? "Click to show individual rights" : undefined}
      >
        <div className="perm-ace-who">
          <span className="perm-ace-name">
            {ace.name}
            {ace.domain && <span className="perm-ace-domain"> ({ace.domain})</span>}
          </span>
          <span className="perm-ace-meta">
            <span>{ace.inherited ? "Inherited" : "Explicit"}</span>
            {ace.appliesTo && <span> · {ace.appliesTo}</span>}
          </span>
        </div>
        <div className="perm-ace-badges">
          <span className="perm-kind" data-kind={ace.kind}>
            {ace.kind === "deny" ? "Deny" : "Allow"}
          </span>
          <span className="perm-level">{ace.level}</span>
          {hasRights && <Icon name={expanded ? "expand" : "chevron"} size={10} />}
        </div>
      </button>
      {expanded && hasRights && (
        <div className="perm-rights">
          {ace.rights.map((r) => (
            <span key={`${index}-${r}`} className="perm-right-chip">
              {r}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

export function PermissionsDialog() {
  const path = useStore((s) => s.permissionsFor);
  const [data, setData] = useState<FilePermissions | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setData(null);
    setError(null);
    setCopied(false);
    if (!path) return;
    let alive = true;
    api
      .getPermissions(path)
      .then((res) => alive && setData(res))
      .catch((err) => alive && setError(String(err)));
    return () => {
      alive = false;
    };
  }, [path]);

  if (!path) return null;

  const close = () => useStore.setState({ permissionsFor: null });
  const copy = () => {
    if (!data) return;
    void navigator.clipboard.writeText(formatSummaryText(data)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    });
  };

  const showGroup = data?.group && data.group !== data.owner && !data.group.endsWith("\\None");

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div
        className="dialog permissions-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Permissions"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          e.stopPropagation();
        }}
      >
        <div className="dialog-title-row">
          <Icon name="shield" size={16} />
          <h3>Permissions</h3>
          <span className="grow" />
          <button className="icon-button" aria-label="Close" onClick={close} autoFocus>
            <Icon name="close" />
          </button>
        </div>

        {!data && !error && <p className="muted">Reading permissions…</p>}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}

        {data && (
          <div className="perm-body">
            <div className="perm-target">
              <FileIcon kind={data.isDir ? "folder" : "file"} name={data.name} size={24} />
              <div className="perm-target-text">
                <strong>{data.name}</strong>
                <span className="muted mono" title={data.path}>
                  {maskAddresses(data.path)}
                </span>
              </div>
              {data.readonly && <span className="perm-tag">Read-only</span>}
            </div>

            <dl className="perm-facts">
              {data.owner && (
                <div>
                  <dt>Owner</dt>
                  <dd>{data.owner}</dd>
                </div>
              )}
              {showGroup && (
                <div>
                  <dt>Group</dt>
                  <dd>{data.group}</dd>
                </div>
              )}
              {data.windows && (
                <div>
                  <dt>Inheritance</dt>
                  <dd>{data.windows.inherited ? "Inherited from parent folder" : "Disabled (explicit only)"}</dd>
                </div>
              )}
              {data.unix && (
                <div>
                  <dt>Mode</dt>
                  <dd className="mono">
                    {data.unix.modeText} ({data.unix.octal})
                    {data.unix.setuid ? " · setuid" : ""}
                    {data.unix.setgid ? " · setgid" : ""}
                    {data.unix.sticky ? " · sticky" : ""}
                  </dd>
                </div>
              )}
            </dl>

            {data.note && <p className="perm-note">{data.note}</p>}

            {data.windows && (
              <div className="perm-section">
                <div className="perm-section-head">
                  <span>Access ({data.windows.entries.length})</span>
                  <span className="muted">Click an entry for details</span>
                </div>
                {data.windows.entries.length === 0 ? (
                  <p className="muted">No access entries are present (no users have access).</p>
                ) : (
                  <div className="perm-list">
                    {data.windows.entries.map((ace, i) => (
                      <AceRow key={`${ace.account}-${ace.kind}-${i}`} ace={ace} index={i} />
                    ))}
                  </div>
                )}
              </div>
            )}

            {data.unix && (
              <table className="perm-unix-table" aria-label="Unix permissions">
                <thead>
                  <tr>
                    <th>Who</th>
                    <th>Read</th>
                    <th>Write</th>
                    <th>Execute</th>
                  </tr>
                </thead>
                <tbody>
                  {(
                    [
                      ["Owner", data.unix.ownerBits],
                      ["Group", data.unix.groupBits],
                      ["Others", data.unix.otherBits],
                    ] as const
                  ).map(([label, bits]) => (
                    <tr key={label}>
                      <td>{label}</td>
                      <td>{bits.read ? <Icon name="check" size={13} /> : <span className="muted">—</span>}</td>
                      <td>{bits.write ? <Icon name="check" size={13} /> : <span className="muted">—</span>}</td>
                      <td>{bits.execute ? <Icon name="check" size={13} /> : <span className="muted">—</span>}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}

        <div className="dialog-buttons perm-footer">
          <span className="muted perm-readonly-hint">Read-only view</span>
          <span className="grow" />
          {data && (
            <button className="secondary" onClick={copy}>
              {copied ? "Copied" : "Copy"}
            </button>
          )}
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
