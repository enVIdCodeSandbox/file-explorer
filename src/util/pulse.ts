import { useEffect, useRef, useState } from "react";

/**
 * A short animation that can be started again while it is still playing. A CSS animation only restarts when its name
 * changes, so each trigger flips between two names ("a" and "b"); the value is undefined once `ms` has passed.
 * Refreshing twice in quick succession therefore plays the whole animation twice instead of being cut off.
 */
export function nextPulse(current: "a" | "b" | undefined): "a" | "b" {
  return current === "a" ? "b" : "a";
}

export function usePulse(trigger: number, ms: number): "a" | "b" | undefined {
  const [pulse, setPulse] = useState<"a" | "b" | undefined>(undefined);
  const last = useRef<"a" | "b" | undefined>(undefined);
  useEffect(() => {
    if (!trigger) return;
    last.current = nextPulse(last.current);
    setPulse(last.current);
    const t = setTimeout(() => setPulse(undefined), ms);
    return () => clearTimeout(t);
  }, [trigger, ms]);
  return pulse;
}
