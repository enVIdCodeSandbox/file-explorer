// Settings > About: the app, the developer, and a detailed read-out of this
// computer. The hardware report is gathered off the UI thread; the page paints
// at once with what is already known and fills in when the report arrives.

import { useCallback, useEffect, useState } from "react";
import { api, type BatteryInfo, type FullReport, type Lifespan, type PhysicalDisk } from "../api";
import { DEVELOPER, PROJECT } from "../about";
import { useStore } from "../store";
import { formatCount, formatDate, formatDuration, formatMemory, formatSize } from "../util/format";

type Row = [label: string, value: React.ReactNode];

function Section({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <section className="about-section">
      <h4>{title}</h4>
      {hint && <p className="muted about-hint">{hint}</p>}
      {children}
    </section>
  );
}

function Facts({ rows }: { rows: (Row | null | false | undefined)[] }) {
  const shown = rows.filter((r): r is Row => !!r && r[1] != null && r[1] !== "");
  if (!shown.length) return <p className="muted">Not available on this computer.</p>;
  return (
    <dl className="details-list">
      {shown.map(([k, v]) => (
        <div key={k} className="about-row">
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

const uptime = (secs: number) => {
  const d = Math.floor(secs / 86400);
  return d >= 1 ? `${d} day${d === 1 ? "" : "s"} ${formatDuration(secs % 86400)}` : formatDuration(secs);
};

function Life({ life }: { life: Lifespan | null }) {
  if (!life) return null;
  return (
    <div className="life" data-basis={life.basis}>
      <strong>Expected life</strong>
      <span>{life.summary}</span>
      <span className="muted">{life.basis === "projection" ? "Projected from the drive's own wear counter." : "A rough guide, not a prediction."}</span>
    </div>
  );
}

function Disk({ d }: { d: PhysicalDisk }) {
  const ok = d.health && /^(healthy|ok|passed|verified)$/i.test(d.health);
  return (
    <div className="disk-card">
      <div className="disk-head">
        <strong>{d.model || d.name}</strong>
        <span className="muted">
          {d.kind}
          {d.size ? ` · ${formatSize(d.size)}` : ""}
        </span>
        {d.health && (
          <span className="health-pill" data-ok={ok ? "yes" : "no"}>
            {d.health}
          </span>
        )}
      </div>
      <Facts
        rows={[
          ["Device", d.name],
          ["Connection", d.bus],
          ["Temperature", d.temperatureC != null && `${Math.round(d.temperatureC)} °C`],
          ["Powered on", d.powerOnHours != null && `${formatCount(d.powerOnHours)} hours (${(d.powerOnHours / 8766).toFixed(1)} years)`],
          ["Wear", d.wearPercent != null && `${d.wearPercent.toFixed(0)}% of rated writes used`],
          ["Data written", d.bytesWritten != null && formatSize(d.bytesWritten)],
          ["Reallocated sectors", d.reallocatedSectors != null && formatCount(d.reallocatedSectors)],
          ["Media errors", d.mediaErrors != null && formatCount(d.mediaErrors)],
        ]}
      />
      <Life life={d.lifespan} />
      <p className="muted about-source">Source: {d.source}</p>
    </div>
  );
}

function Battery({ b }: { b: BatteryInfo }) {
  return (
    <>
      <Facts
        rows={[
          ["Charge", `${Math.round(b.percent)}% · ${b.state.toLowerCase()}`],
          ["Health", b.healthPercent != null && `${b.healthPercent.toFixed(0)}% of its original capacity`],
          ["Charge cycles", b.cycles != null && formatCount(b.cycles)],
          ["Capacity", b.fullWh != null && `${b.fullWh.toFixed(1)} Wh${b.designWh ? ` of ${b.designWh.toFixed(1)} Wh when new` : ""}`],
          ["Make and model", [b.vendor, b.model].filter(Boolean).join(" ")],
          ["Chemistry", b.technology],
        ]}
      />
      <Life life={b.lifespan} />
    </>
  );
}

function asText(r: FullReport): string {
  const { app, system: s } = r;
  const line = (k: string, v: unknown) => (v == null || v === "" || v === false ? "" : `${k}: ${v}\n`);
  return [
    `${PROJECT.name} ${app.version} (${app.profile}, ${app.os}/${app.arch}, Tauri ${app.tauri}, web view ${app.webview ?? "unknown"})\n\n`,
    "System\n",
    line("  OS", `${s.os.name} ${s.os.version} (kernel ${s.os.kernel})`),
    line("  Computer", [s.machine.manufacturer, s.machine.model].filter(Boolean).join(" ")),
    line("  Board", s.machine.board),
    line("  Firmware", s.machine.firmware),
    line("  Uptime", uptime(s.os.uptimeSecs)),
    line("  CPU", `${s.cpu.brand} (${s.cpu.physicalCores ?? "?"} cores, ${s.cpu.logical} threads)`),
    line("  Memory", `${formatMemory(s.memory.total)} (${formatSize(s.memory.available)} free)`),
    ...s.gpus.map((g) => line("  GPU", g)),
    "\nDrives\n",
    ...s.disks.map(
      (d) =>
        `  ${d.model} · ${d.kind} · ${formatSize(d.size)} · health ${d.health ?? "unknown"}` +
        `${d.wearPercent != null ? ` · wear ${d.wearPercent}%` : ""}${d.powerOnHours != null ? ` · ${d.powerOnHours} h` : ""}` +
        `${d.lifespan ? `\n    ${d.lifespan.summary}` : ""}\n`,
    ),
  ].join("");
}

export function About() {
  const info = useStore((s) => s.info);
  const budget = useStore((s) => s.budget);
  const index = useStore((s) => s.indexStatus);
  const drives = useStore((s) => s.drives);
  const { showToast } = useStore.getState();
  const [report, setReport] = useState<FullReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setBusy(true);
    setError(null);
    api
      .systemReport()
      .then(setReport)
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  }, []);
  useEffect(load, [load]);

  if (!info || !budget) return null;
  const tier = { full: "Full", balanced: "Balanced", saver: "Power saver", manual: "Manual" }[budget.tier];
  const s = report?.system;
  const loading = busy && !report;
  const dev = DEVELOPER;
  const hasDev = dev.name || dev.bio || dev.links.length > 0;

  return (
    <div className="about">
      <div className="about-hero">
        <div>
          <h2>{PROJECT.name}</h2>
          <p className="muted">{PROJECT.tagline}</p>
        </div>
        <span className="version-pill">Version {info.version}</span>
      </div>

      <div className="button-row">
        <button className="secondary" disabled={busy} onClick={load}>
          {busy ? "Reading this computer…" : "Refresh"}
        </button>
        <button
          className="secondary"
          disabled={!report}
          onClick={() => report && void navigator.clipboard.writeText(asText(report)).then(() => showToast("System report copied."))}
        >
          Copy report
        </button>
      </div>
      {error && <p className="error-text">Could not read the system report: {error}</p>}
      {loading && <p className="muted">Reading hardware and drive health. This can take a few seconds the first time.</p>}

      <Section title="This app">
        <Facts
          rows={[
            ["Version", `${info.version}${report ? ` (${report.app.profile} build)` : ""}`],
            ["License", report?.app.license ? `${PROJECT.licenseName} (${report.app.license})` : PROJECT.licenseName],
            ["Platform", report && `${report.app.os} ${report.app.arch}`],
            ["Shell", report && `Tauri ${report.app.tauri}`],
            ["Web view", report?.app.webview],
            ["Running for", report && formatDuration(report.app.startedSecsAgo)],
            ["Memory used by the app", s && s.software.ownMemory > 0 && formatSize(s.software.ownMemory)],
            ["Settings folder", report && <span className="path">{report.app.settingsDir}</span>],
            ["Cache folder", report && <span className="path">{report.app.cacheDir}</span>],
          ]}
        />
        <dl className="details-list">
          {PROJECT.built.map(([k, v]) => (
            <div key={k} className="about-row">
              <dt>{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
      </Section>

      <Section title="About the developer">
        {hasDev ? (
          <>
            <Facts rows={[["Name", dev.name], ["Role", dev.role]]} />
            {dev.bio && <p>{dev.bio}</p>}
            {dev.links.length > 0 && (
              <div className="button-row">
                {dev.links.map((l) => (
                  <button key={l.url} className="secondary" onClick={() => void api.openItem(l.url).catch((e) => showToast(String(e)))}>
                    {l.label}
                  </button>
                ))}
              </div>
            )}
          </>
        ) : (
          <p className="muted">Developer details are set in src/about.ts.</p>
        )}
      </Section>

      <Section title="Operating system">
        <Facts
          rows={[
            ["System", s ? `${s.os.name}` : info.os],
            ["Version", s?.os.version],
            ["Kernel", s?.os.kernel],
            ["Architecture", s?.os.arch],
            ["Computer name", s?.os.host],
            ["Desktop", s?.os.desktop],
            ["Running for", s && uptime(s.os.uptimeSecs)],
          ]}
        />
      </Section>

      <Section title="Computer and processor">
        <Facts
          rows={[
            ["Make and model", [s?.machine.manufacturer, s?.machine.model].filter(Boolean).join(" ")],
            ["Motherboard", s?.machine.board],
            ["Firmware", s?.machine.firmware],
            ["Processor", s?.cpu.brand],
            ["Vendor", s?.cpu.vendor],
            ["Cores and threads", s ? `${s.cpu.physicalCores ?? "?"} cores · ${s.cpu.logical} threads` : `${info.system.cpus} threads`],
            ["Speed", s && s.cpu.mhz > 0 && `${(s.cpu.mhz / 1000).toFixed(2)} GHz`],
          ]}
        />
      </Section>

      <Section title="Memory">
        <Facts
          rows={[
            ["Installed", formatMemory((s?.memory.total ?? info.system.totalRamMb * 1024 * 1024))],
            ["Free right now", formatSize((s?.memory.available ?? info.system.availableRamMb * 1024 * 1024))],
            ["Swap or page file", s && s.memory.swapTotal > 0 && `${formatSize(s.memory.swapUsed)} used of ${formatSize(s.memory.swapTotal)}`],
            ["Modules", s && s.memory.modules.length > 0 && s.memory.modules.join(" · ")],
          ]}
        />
      </Section>

      {s && s.gpus.length > 0 && (
        <Section title="Graphics">
          <Facts rows={s.gpus.map((g, i) => [s.gpus.length > 1 ? `Adapter ${i + 1}` : "Adapter", g] as Row)} />
        </Section>
      )}

      {s?.battery && (
        <Section title="Battery">
          <Battery b={s.battery} />
        </Section>
      )}

      <Section title="Physical drives" hint="Health, wear and expected life as reported by the drives themselves.">
        {loading && <p className="muted">Reading drive health…</p>}
        {s && s.disks.length === 0 && <p className="muted">No physical drives could be listed.</p>}
        {s?.disks.map((d) => <Disk key={d.name + d.model} d={d} />)}
        {s?.notes.map((n) => (
          <p key={n} className="muted about-note">
            {n}
          </p>
        ))}
      </Section>

      <Section title="Volumes">
        {drives.length === 0 && <p className="muted">None.</p>}
        {drives.map((d) => (
          <div key={d.mount} className="volume-line">
            <strong>{d.mount}</strong>
            <span className="muted">
              {d.fs} · {d.media}
            </span>
            <span className="grow" />
            <span>{d.total ? `${formatSize(d.total - d.available)} used of ${formatSize(d.total)}` : "no size"}</span>
          </div>
        ))}
      </Section>

      <Section title="Software running now">
        <Facts
          rows={[
            ["Processes", s && formatCount(s.software.processCount)],
            [
              "Using the most memory",
              s && s.software.biggest.length > 0 && (
                <ul className="proc-list">
                  {s.software.biggest.map((p) => (
                    <li key={p.name}>
                      <span>{p.name}</span>
                      <span className="muted">{formatSize(p.memory)}</span>
                    </li>
                  ))}
                </ul>
              ),
            ],
          ]}
        />
      </Section>

      <Section title="How the app is running">
        <Facts
          rows={[
            ["Resource mode", `${tier}: ${budget.ioThreads} transfer threads, ${budget.indexThreads} indexing, ${budget.thumbThreads} thumbnail`],
            ["Cache", `${budget.cacheMb} MB`],
            ["Background indexing", budget.backgroundIndexing ? "on" : "off"],
            ["Preloading", budget.prefetch ? "on" : "off"],
            [
              "File index",
              index
                ? `${index.state} · ${formatCount(index.entries)} items · ${formatSize(index.memoryBytes)} in memory${
                    index.builtAt ? ` · updated ${formatDate(index.builtAt * 1000)}` : ""
                  }${index.truncated ? " · limited by the cache size" : ""}`
                : "unknown",
            ],
            ["Indexed folders", index?.roots.length ? <span className="path">{index.roots.join(", ")}</span> : null],
          ]}
        />
        <div className="button-row">
          <button className="secondary" onClick={() => void api.reindex().then(() => showToast("Rebuilding the file index…"))}>
            Rebuild index now
          </button>
        </div>
      </Section>
    </div>
  );
}
