import { useEffect, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { Icon } from "./Icons";

/** The strip shown while a form waits for a file to be chosen (see util/pickFile.ts). */
export function PickBar() {
  const picker = useStore((s) => s.picker);
  const selection = useStore((s) => s.selection);
  const here = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location);
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setTyped("");
    setError(null);
  }, [picker]);
  useEffect(() => {
    if (!picker) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !(e.target as HTMLElement | null)?.closest?.("input") && picker.done(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [picker]);
  if (!picker) return null;

  const choose = async (path: string) => {
    try {
      const r = await api.resolvePath(path);
      if (folder && !r.isDir) return setError("That is a file. Choose a folder or drive.");
      if (!folder && r.isDir) return setError("That is a folder. Open it and choose a file inside.");
      picker.done(r.path);
    } catch {
      setError("That file could not be found.");
    }
  };
  const folder = picker.kind === "folder";
  // For a folder, nothing selected means the folder being shown.
  const one = selection.length === 1 ? selection[0] : folder ? here ?? null : null;

  return (
    <div className="pick-bar" role="region" aria-label={picker.title}>
      <Icon name="info" size={16} />
      <div className="pick-text">
        <strong>{picker.title}</strong>
        <span>
          {folder
            ? "Open the folder you want (or select it) and press Choose, or type its full path. Choose with nothing selected uses the folder shown."
            : "Browse to the file and open it (or select it and press Choose), or type its full path."}
        </span>
      </div>
      <input
        className="text-input pick-path"
        value={typed}
        placeholder="Type a full path, then press Enter"
        spellCheck={false}
        onChange={(e) => (setTyped(e.target.value), setError(null))}
        onKeyDown={(e) => {
          if (e.key === "Enter" && typed.trim()) void choose(typed.trim());
          e.stopPropagation();
        }}
      />
      <button className="primary" disabled={!one} title={one ?? (folder ? "Open a folder first" : "Select one file first")} onClick={() => one && void choose(one)}>
        Choose
      </button>
      <button onClick={() => picker.done(null)}>Cancel</button>
      {error && <span className="pick-error" role="alert">{error}</span>}
    </div>
  );
}
