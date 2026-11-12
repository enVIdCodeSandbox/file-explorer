// The documents shown in Settings > Documentation. They are the Markdown files
// in /docs, bundled at build time, so the in-app text can never drift from the
// repository. To add a page: put a .md file in /docs and list it here.

import userGuide from "../../docs/USER-GUIDE.md?raw";
import developerGuide from "../../docs/DEVELOPER-GUIDE.md?raw";
import architecture from "../../docs/ARCHITECTURE.md?raw";
import transfers from "../../docs/TRANSFERS.md?raw";
import iface from "../../docs/INTERFACE.md?raw";
import ipc from "../../docs/IPC.md?raw";
import theming from "../../docs/THEMING.md?raw";
import roadmap from "../../docs/ROADMAP.md?raw";

export interface DocPage {
  id: string;
  group: "Using the app" | "Developers";
  title: string;
  /** The file name other pages use when they link to this one. */
  file: string;
  text: string;
}

export const DOC_PAGES: DocPage[] = [
  { id: "guide", group: "Using the app", title: "User guide", file: "USER-GUIDE.md", text: userGuide },
  { id: "themes", group: "Using the app", title: "Themes and custom CSS", file: "THEMING.md", text: theming },
  { id: "plans", group: "Using the app", title: "Roadmap and known limits", file: "ROADMAP.md", text: roadmap },
  { id: "dev", group: "Developers", title: "Developer guide", file: "DEVELOPER-GUIDE.md", text: developerGuide },
  { id: "arch", group: "Developers", title: "Architecture", file: "ARCHITECTURE.md", text: architecture },
  { id: "transfers", group: "Developers", title: "Transfers", file: "TRANSFERS.md", text: transfers },
  { id: "interface", group: "Developers", title: "Interface internals", file: "INTERFACE.md", text: iface },
  { id: "ipc", group: "Developers", title: "Command and event reference", file: "IPC.md", text: ipc },
];

/** Search every page: a page matches when all words appear in one section. */
export interface DocHit {
  page: DocPage;
  heading: string;
  anchor: number; // index of the section in the page
  snippet: string;
}

export function sectionsOf(text: string): { heading: string; body: string }[] {
  const out: { heading: string; body: string }[] = [];
  let heading = "";
  let body: string[] = [];
  let fence = false;
  for (const line of text.split("\n")) {
    if (line.startsWith("```")) fence = !fence;
    if (!fence && /^#{1,3} /.test(line)) {
      out.push({ heading, body: body.join("\n") });
      heading = line.replace(/^#+ /, "");
      body = [];
    } else body.push(line);
  }
  out.push({ heading, body: body.join("\n") });
  return out.filter((s) => s.heading || s.body.trim());
}

export function searchDocs(query: string, pages = DOC_PAGES, limit = 40): DocHit[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const hits: (DocHit & { score: number })[] = [];
  for (const page of pages) {
    sectionsOf(page.text).forEach((sec, anchor) => {
      const hay = `${sec.heading}\n${sec.body}`.toLowerCase();
      if (!words.every((w) => hay.includes(w))) return;
      const at = hay.indexOf(words[0]) - sec.heading.length - 1;
      const body = sec.body.replace(/[`*|]|-{3,}/g, "").replace(/\s+/g, " ").trim();
      const start = Math.max(0, at - 40);
      const snippet = (start > 0 ? "…" : "") + body.slice(start, start + 140) + (start + 140 < body.length ? "…" : "");
      const inHeading = words.filter((w) => sec.heading.toLowerCase().includes(w)).length;
      hits.push({ page, heading: sec.heading || page.title, anchor, snippet, score: inHeading * 10 + (sec.heading ? 1 : 0) });
    });
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, limit);
}
