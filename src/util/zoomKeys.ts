// Ctrl + and Ctrl - (and Ctrl 0): the keys that go with Ctrl + scroll for making things bigger or smaller.

export interface ZoomKey {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

/** 1 = bigger, -1 = smaller, 0 = back to normal, null = not a zoom key. */
export function zoomStep(e: ZoomKey): 1 | -1 | 0 | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "NumpadAdd" || e.code === "Equal") return 1;
  if (e.key === "-" || e.key === "_" || e.code === "NumpadSubtract" || e.code === "Minus") return -1;
  if (e.key === "0" || e.code === "Numpad0" || e.code === "Digit0") return 0;
  return null;
}

/** Wheel notches: collects small deltas (trackpads) and says how many whole steps they add up to. */
export function wheelSteps(carry: number, deltaY: number, notch = 60): { steps: number; carry: number } {
  let total = carry + deltaY;
  let steps = 0;
  while (Math.abs(total) >= notch) {
    const dir = total < 0 ? 1 : -1; // wheel up = bigger
    steps += dir;
    total += dir * notch;
  }
  return { steps, carry: total };
}
