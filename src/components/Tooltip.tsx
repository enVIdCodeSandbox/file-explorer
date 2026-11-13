// Tooltips in the app's own style. The browser's built-in ones are plain black
// boxes on Linux, stay on screen over open menus, and swallow clicks. This takes
// over every `title` attribute on the page: the first time the pointer rests on
// an element its title moves to `data-tip` (so the browser stops drawing it) and
// is shown here after a short delay. It hides on any click, key press or scroll.

import { useEffect, useState } from "react";

interface Tip {
  text: string;
  x: number;
  top: number;
  bottom: number;
}

export function Tooltips() {
  const [tip, setTip] = useState<Tip | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let current: HTMLElement | null = null;
    const hide = () => {
      clearTimeout(timer);
      setTip(null);
    };
    const over = (e: PointerEvent) => {
      const el = (e.target as Element | null)?.closest?.("[title],[data-tip]") as HTMLElement | null;
      if (el === current) return;
      current = el;
      hide();
      if (!el) return;
      const title = el.getAttribute("title");
      if (title) {
        el.setAttribute("data-tip", title);
        el.removeAttribute("title");
        // Icon-only controls keep a name for screen readers.
        if (!el.hasAttribute("aria-label") && !el.textContent?.trim()) el.setAttribute("aria-label", title);
      }
      const text = el.getAttribute("data-tip");
      if (!text) return;
      timer = setTimeout(() => {
        if (!el.isConnected) return;
        const r = el.getBoundingClientRect();
        setTip({ text, x: r.left + r.width / 2, top: r.top, bottom: r.bottom });
      }, 550);
    };
    const leave = (e: PointerEvent) => {
      if (!e.relatedTarget) {
        current = null;
        hide();
      }
    };
    document.addEventListener("pointerover", over, true);
    document.addEventListener("pointerout", leave, true);
    for (const t of ["pointerdown", "keydown", "wheel", "contextmenu", "dragstart"]) document.addEventListener(t, hide, true);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("pointerover", over, true);
      document.removeEventListener("pointerout", leave, true);
      for (const t of ["pointerdown", "keydown", "wheel", "contextmenu", "dragstart"]) document.removeEventListener(t, hide, true);
    };
  }, []);

  if (!tip) return null;
  const below = tip.bottom + 40 < window.innerHeight;
  const half = 160;
  return (
    <div
      className="tooltip"
      role="tooltip"
      style={{ left: Math.min(Math.max(tip.x, half + 6), window.innerWidth - half - 6), top: below ? tip.bottom + 6 : undefined, bottom: below ? undefined : window.innerHeight - tip.top + 6 }}
    >
      {tip.text}
    </div>
  );
}
