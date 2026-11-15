// Type-to-find: the item whose name comes next alphabetically at or after what was typed.

const same = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });

/**
 * Index (in `names`' own order) of the closest match to `text`: the alphabetically first name
 * that is not before the text, so a name starting with the text wins over one that merely
 * sorts later. When everything sorts before the text, the last one alphabetically.
 */
export function closestIndex(names: string[], text: string): number {
  if (!names.length || !text) return -1;
  let best = -1;
  let last = -1;
  for (let i = 0; i < names.length; i++) {
    if (last < 0 || same(names[i], names[last]) > 0) last = i;
    if (same(names[i], text) >= 0 && (best < 0 || same(names[i], names[best]) < 0)) best = i;
  }
  return best >= 0 ? best : last;
}

/** What the typed text becomes after a key, or null when the key is not part of typing. */
export function nextText(text: string, key: string): string | null {
  if (key === "Backspace") return text.slice(0, -1);
  if (key.length === 1) return text + key.toLowerCase();
  return null;
}
