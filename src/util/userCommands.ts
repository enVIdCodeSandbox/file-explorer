// User-defined commands (button bar / Commands menu) and the command line: placeholders and splitting.
//
//   %P  the folder being viewed, with a closing slash        %p  the same without it
//   %N  the name of the item under the cursor                %n  that name without its extension
//   %E  the extension (no dot)                               %F  the full path of the item under the cursor
//   %S  the names of the selected items                      %L  the full paths of the selected items
//   %D  the name of the folder being viewed                  %%  a percent sign
// Names with spaces are put in quotes unless they are inside quotes already. %S and %L give one quoted item each,
// the cursor item if nothing is selected.

export interface CommandContext {
  /** The folder being viewed. */
  folder: string;
  /** The item under the cursor, if any. */
  cursor: { name: string; isDir: boolean } | null;
  /** The selected items (names, in the folder). */
  selected: string[];
}

const sepOf = (p: string) => (p.includes("\\") && !p.includes("/") ? "\\" : p.startsWith("/") ? "/" : p.includes("\\") ? "\\" : "/");
const quote = (s: string) => `"${s}"`;
const needsQuote = (s: string) => /[\s"]/.test(s) || s === "";

export function joinPath(folder: string, name: string): string {
  if (!folder) return name;
  const sep = sepOf(folder);
  return folder.endsWith("/") || folder.endsWith("\\") ? folder + name : folder + sep + name;
}

function splitExt(name: string): [string, string] {
  const i = name.lastIndexOf(".");
  return i > 0 ? [name.slice(0, i), name.slice(i + 1)] : [name, ""];
}

/** Replace the placeholders in `text`. */
export function expandPlaceholders(text: string, ctx: CommandContext): string {
  const folder = ctx.folder.replace(/[\\/]+$/, "") || ctx.folder;
  const sep = sepOf(ctx.folder);
  const cur = ctx.cursor?.name ?? "";
  const [stem, ext] = splitExt(cur);
  const chosen = ctx.selected.length ? ctx.selected : cur ? [cur] : [];
  let inQuote = false;
  let out = "";
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      inQuote = !inQuote;
      out += c;
      continue;
    }
    if (c !== "%" || i + 1 >= text.length) {
      out += c;
      continue;
    }
    const k = text[i + 1];
    const single = (v: string) => (inQuote || !needsQuote(v) ? v : quote(v));
    let rep: string | null = null;
    switch (k) {
      case "%": rep = "%"; break;
      case "P": rep = single(folder.endsWith(sep) ? folder : folder + sep); break;
      case "p": rep = single(folder); break;
      case "N": rep = single(cur); break;
      case "n": rep = single(stem); break;
      case "E": rep = ext; break;
      case "F": rep = single(cur ? joinPath(ctx.folder, cur) : ""); break;
      case "D": rep = single(folder.split(/[\\/]/).filter(Boolean).pop() ?? ""); break;
      case "S": rep = chosen.map((n) => (inQuote ? n : quote(n))).join(inQuote ? " " : " "); break;
      case "L": rep = chosen.map((n) => (inQuote ? joinPath(ctx.folder, n) : quote(joinPath(ctx.folder, n)))).join(" "); break;
    }
    if (rep === null) {
      out += c;
      continue;
    }
    out += rep;
    i++;
  }
  return out;
}

/** Split a command line into words, the way a shell would for plain quotes (no variables, no escapes). */
export function splitWords(text: string): string[] {
  const out: string[] = [];
  let cur = "";
  let had = false;
  let q: string | null = null;
  for (const c of text) {
    if (q) {
      if (c === q) q = null;
      else cur += c;
    } else if (c === '"' || c === "'") {
      q = c;
      had = true;
    } else if (/\s/.test(c)) {
      if (had || cur) out.push(cur);
      cur = "";
      had = false;
    } else cur += c;
  }
  if (had || cur) out.push(cur);
  return out;
}

/** What a button press runs: the program, its arguments, the folder. */
export function buildRun(cmd: { program: string; params: string; startDir: string }, ctx: CommandContext) {
  return {
    program: expandPlaceholders(cmd.program, ctx).replace(/^"|"$/g, ""),
    args: splitWords(expandPlaceholders(cmd.params, ctx)),
    cwd: cmd.startDir.trim() ? expandPlaceholders(cmd.startDir, ctx).replace(/^"|"$/g, "") : ctx.folder,
    line: `${expandPlaceholders(cmd.program, ctx)} ${expandPlaceholders(cmd.params, ctx)}`.trim(),
  };
}

/** Put a name or a path into the command line at the caret (Ctrl+Enter / Ctrl+Shift+Enter), with a space before when needed. */
export function insertAtCaret(text: string, caret: number, insert: string): { text: string; caret: number } {
  const value = needsQuote(insert) ? quote(insert) : insert;
  const before = text.slice(0, caret);
  const lead = before && !/\s$/.test(before) ? " " : "";
  const add = `${lead}${value} `;
  return { text: before + add + text.slice(caret), caret: caret + add.length };
}
