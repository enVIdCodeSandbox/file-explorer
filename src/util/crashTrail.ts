// The trail the crash log is made of: what the person did and which commands ran, sent to the
// backend in small batches so it is on disk even if the window dies a moment later.
import { invoke as rawInvoke } from "@tauri-apps/api/core";

let queue: string[] = [];
let timer: ReturnType<typeof setTimeout> | undefined;
let enabled = true;

/** Commands that run constantly; leaving them out keeps the trail about what matters. */
const NOISY = new Set(["thumbnail", "folder_fill", "thumbs_prewarm", "crash_note", "crash_context", "crash_logs", "crash_report", "progress_poll", "hardware_snapshot"]);

function send() {
  timer = undefined;
  if (!queue.length) return;
  const lines = queue;
  queue = [];
  void rawInvoke("crash_note", { lines }).catch(() => {});
}

/** Writes the trail now (before something risky, like handing a drag to the system). */
export function flushTrail() {
  clearTimeout(timer);
  send();
}

export function crumb(text: string) {
  if (!enabled) return;
  queue.push(text);
  if (queue.length > 100) queue.shift();
  timer ??= setTimeout(send, 300);
}

/** `invoke`, with a line in the trail when a command starts and when it ends. */
export async function tracedInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (NOISY.has(cmd)) return rawInvoke<T>(cmd, args);
  const t0 = performance.now();
  crumb(`-> ${cmd}`);
  try {
    const r = await rawInvoke<T>(cmd, args);
    crumb(`<- ${cmd} ${Math.round(performance.now() - t0)}ms`);
    return r;
  } catch (e) {
    crumb(`!! ${cmd} ${String(e).slice(0, 160)}`);
    throw e;
  }
}

/** What an element is, for the trail: its role and label, never the text of a file. */
export function describe(el: Element | null): string {
  const hit = el?.closest("button,[role=menuitem],[role=tab],[role=option],a,input,select,[data-cmd],[data-crumb],[draggable=true],tr,li");
  if (!hit) return el ? el.tagName.toLowerCase() : "?";
  const h = hit as HTMLElement;
  const tag = h.tagName.toLowerCase();
  const label = h.getAttribute("aria-label") || h.getAttribute("data-cmd") || h.getAttribute("data-crumb") || h.getAttribute("title");
  const isItem = tag === "tr" || tag === "li" || h.hasAttribute("draggable");
  const text = !label && !isItem && (tag === "button" || tag === "a" || h.getAttribute("role")) ? (h.textContent ?? "").trim().slice(0, 40) : "";
  const cls = (h.className && typeof h.className === "string" ? h.className : "").split(/\s+/).filter(Boolean).slice(0, 2).join(".");
  return `${isItem ? "item" : tag}${label ? ` "${label.slice(0, 50)}"` : text ? ` "${text}"` : ""}${cls ? ` .${cls}` : ""}`;
}

/** Listens for clicks, key presses and errors. `state` supplies a one-line picture of the window. */
export function installTrail(state: () => string, report: (kind: string, message: string) => void, on: () => boolean) {
  const gate = () => (enabled = on());
  gate();
  const click = (e: MouseEvent) => crumb(`click ${e.button ? `(button ${e.button}) ` : ""}${describe(e.target as Element)}`);
  const key = (e: KeyboardEvent) => {
    const special = e.ctrlKey || e.metaKey || e.altKey || e.key.length > 1;
    if (!special || ["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;
    crumb(`key ${e.ctrlKey ? "Ctrl+" : ""}${e.metaKey ? "Meta+" : ""}${e.altKey ? "Alt+" : ""}${e.shiftKey ? "Shift+" : ""}${e.key}`);
  };
  const drag = (e: DragEvent) => crumb(`${e.type} ${e.type === "dragstart" ? describe(e.target as Element) : ""}`.trim());
  const err = (e: ErrorEvent) => {
    crumb(`!! error: ${e.message}`);
    report("Page error", `${e.message}\n${e.filename}:${e.lineno}:${e.colno}\n${e.error?.stack ?? ""}`);
  };
  const rej = (e: PromiseRejectionEvent) => {
    const r = e.reason;
    crumb(`!! unhandled rejection: ${String(r).slice(0, 160)}`);
    // A rejected command is already in the trail; only real bugs deserve a report of their own.
    if (r instanceof Error) report("Unhandled promise rejection", `${r.message}\n${r.stack ?? ""}`);
  };
  window.addEventListener("click", click, true);
  window.addEventListener("keydown", key, true);
  window.addEventListener("dragstart", drag, true);
  window.addEventListener("drop", drag, true);
  window.addEventListener("dragend", drag, true);
  window.addEventListener("error", err);
  window.addEventListener("unhandledrejection", rej);
  window.addEventListener("focus", () => crumb("window focused"));
  window.addEventListener("blur", () => crumb("window lost focus"));
  document.addEventListener("visibilitychange", () => crumb(`page ${document.visibilityState}`));
  let last = "";
  const poll = setInterval(() => {
    gate();
    const s = state();
    if (s !== last) {
      last = s;
      void rawInvoke("crash_context", { text: s }).catch(() => {});
    }
  }, 3000);
  return () => clearInterval(poll);
}
