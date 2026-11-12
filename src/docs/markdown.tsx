// A small Markdown renderer for the bundled documentation: headings, paragraphs,
// bullet and numbered lists, tables, fenced code, block quotes, and inline code,
// bold, italic and links. It only produces React elements, never raw HTML, so
// document text cannot inject markup.

import type { ReactNode } from "react";

type Block =
  | { t: "h"; level: number; text: string }
  | { t: "p"; text: string }
  | { t: "ul" | "ol"; items: string[] }
  | { t: "code"; text: string }
  | { t: "quote"; text: string }
  | { t: "table"; head: string[]; rows: string[][] };

const cells = (line: string) => line.trim().replace(/^\||\|$/g, "").split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));

export function parse(md: string): Block[] {
  const lines = md.replace(/\r/g, "").split("\n");
  const out: Block[] = [];
  for (let i = 0; i < lines.length; ) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    if (line.startsWith("```")) {
      const buf: string[] = [];
      for (i++; i < lines.length && !lines[i].startsWith("```"); i++) buf.push(lines[i]);
      i++;
      out.push({ t: "code", text: buf.join("\n") });
      continue;
    }
    const h = /^(#{1,4}) (.*)$/.exec(line);
    if (h) { out.push({ t: "h", level: h[1].length, text: h[2] }); i++; continue; }
    if (line.startsWith("|") && lines[i + 1]?.match(/^\|?\s*:?-{2,}/)) {
      const head = cells(line);
      const rows: string[][] = [];
      for (i += 2; i < lines.length && lines[i].startsWith("|"); i++) rows.push(cells(lines[i]));
      out.push({ t: "table", head, rows });
      continue;
    }
    if (line.startsWith(">")) {
      const buf: string[] = [];
      for (; i < lines.length && lines[i].startsWith(">"); i++) buf.push(lines[i].replace(/^>\s?/, ""));
      out.push({ t: "quote", text: buf.join(" ") });
      continue;
    }
    const list = /^(\s*)([-*]|\d+\.) (.*)$/.exec(line);
    if (list) {
      const ordered = /\d/.test(list[2]);
      const items: string[] = [];
      for (; i < lines.length; i++) {
        const m = /^(\s*)([-*]|\d+\.) (.*)$/.exec(lines[i]);
        if (m) items.push(m[3]);
        else if (/^\s+\S/.test(lines[i]) && items.length) items[items.length - 1] += " " + lines[i].trim();
        else break;
      }
      out.push({ t: ordered ? "ol" : "ul", items });
      continue;
    }
    const buf: string[] = [];
    for (; i < lines.length && lines[i].trim() && !/^(#{1,4} |```|\||>|\s*([-*]|\d+\.) )/.test(lines[i]); i++) buf.push(lines[i].trim());
    if (!buf.length) { buf.push(line.trim()); i++; }
    out.push({ t: "p", text: buf.join(" ") });
  }
  return out;
}

/** `code`, **bold**, *italic*, [text](target). */
export function inline(text: string, open: (target: string) => void, key = ""): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\s][^*]*)\*|\[([^\]]+)\]\(([^)]+)\)/g;
  let last = 0;
  let n = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const k = `${key}-${n++}`;
    if (m[1] !== undefined) out.push(<code key={k}>{m[1]}</code>);
    else if (m[2] !== undefined) out.push(<strong key={k}>{inline(m[2], open, k)}</strong>);
    else if (m[3] !== undefined) out.push(<em key={k}>{inline(m[3], open, k)}</em>);
    else out.push(<a key={k} href="#" onClick={(e) => { e.preventDefault(); open(m[5]); }}>{m[4]}</a>);
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function slug(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

export function Markdown({ text, open }: { text: string; open: (target: string) => void }) {
  return (
    <>
      {parse(text).map((b, i) => {
        const k = String(i);
        switch (b.t) {
          case "h": {
            const Tag = `h${Math.min(6, b.level + 1)}` as "h2";
            return <Tag key={k} id={`doc-${slug(b.text)}`}>{inline(b.text, open, k)}</Tag>;
          }
          case "p": return <p key={k}>{inline(b.text, open, k)}</p>;
          case "ul": return <ul key={k}>{b.items.map((t, j) => <li key={j}>{inline(t, open, `${k}-${j}`)}</li>)}</ul>;
          case "ol": return <ol key={k}>{b.items.map((t, j) => <li key={j}>{inline(t, open, `${k}-${j}`)}</li>)}</ol>;
          case "code": return <pre key={k}><code>{b.text}</code></pre>;
          case "quote": return <blockquote key={k}>{inline(b.text, open, k)}</blockquote>;
          case "table":
            return (
              <div className="doc-table" key={k}>
                <table>
                  <thead><tr>{b.head.map((c, j) => <th key={j}>{inline(c, open, `${k}h${j}`)}</th>)}</tr></thead>
                  <tbody>{b.rows.map((r, j) => <tr key={j}>{r.map((c, l) => <td key={l}>{inline(c, open, `${k}-${j}-${l}`)}</td>)}</tr>)}</tbody>
                </table>
              </div>
            );
        }
      })}
    </>
  );
}
