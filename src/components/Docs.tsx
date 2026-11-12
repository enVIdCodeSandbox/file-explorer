// Settings > Documentation: the user guide and the developer documentation,
// bundled with the app, with search across all of it.

import { useEffect, useMemo, useRef, useState } from "react";
import { DOC_PAGES, searchDocs, type DocPage } from "../docs/pages";
import { Markdown, slug } from "../docs/markdown";

export function Docs() {
  const [pageId, setPageId] = useState(DOC_PAGES[0].id);
  const [query, setQuery] = useState("");
  const [jump, setJump] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);
  const page = DOC_PAGES.find((p) => p.id === pageId) ?? DOC_PAGES[0];
  const hits = useMemo(() => searchDocs(query), [query]);

  // Links between documents name a file ("TRANSFERS.md") and optionally a heading ("#pause").
  const open = (target: string) => {
    const [file, hash] = target.split("#");
    if (/^https?:/.test(target)) { window.open(target, "_blank", "noopener"); return; }
    const to = file ? DOC_PAGES.find((p) => p.file === file.replace(/^.*\//, "")) : page;
    if (to) { setPageId(to.id); setJump(hash ?? null); }
  };

  useEffect(() => {
    const el = jump ? body.current?.querySelector(`#doc-${CSS.escape(jump)}`) : null;
    if (el) el.scrollIntoView({ block: "start" });
    else if (body.current) body.current.scrollTop = 0;
  }, [pageId, jump]);

  const groups = ["Using the app", "Developers"] as const;
  const show = (p: DocPage, heading?: string) => {
    setPageId(p.id);
    setJump(heading ? slug(heading) : null);
    setQuery("");
  };

  return (
    <div className="docs">
      <nav className="docs-nav" aria-label="Documentation">
        <input className="text-input" type="search" placeholder="Search the documentation" aria-label="Search the documentation" value={query} onChange={(e) => setQuery(e.target.value)} />
        {query.trim() ? (
          <div className="docs-hits">
            {hits.length === 0 && <p className="muted">Nothing matches.</p>}
            {hits.map((h, i) => (
              <button key={i} className="docs-hit" onClick={() => show(h.page, h.heading === h.page.title ? undefined : h.heading)}>
                <strong>{h.heading}</strong>
                <span className="muted">{h.page.title}</span>
                <span className="docs-snippet">{h.snippet}</span>
              </button>
            ))}
          </div>
        ) : (
          groups.map((g) => (
            <div key={g}>
              <h5>{g}</h5>
              {DOC_PAGES.filter((p) => p.group === g).map((p) => (
                <button key={p.id} className="docs-page" data-on={p.id === pageId || undefined} onClick={() => show(p)}>
                  {p.title}
                </button>
              ))}
            </div>
          ))
        )}
      </nav>
      <div className="docs-body" ref={body}>
        <article className="doc-article">
          <Markdown text={page.text} open={open} />
        </article>
      </div>
    </div>
  );
}
