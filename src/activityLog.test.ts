import { describe, expect, it } from "vitest";
import { age, filterEvents } from "./components/ActivityLogPanel";
import type { ActivityEvent } from "./api";

const ev = (seq: number, level: ActivityEvent["level"], what: string, detail = ""): ActivityEvent => ({ seq, atMs: 0, level, area: "command", what, detail, tookMs: null });

describe("activity log filters", () => {
  const all = [ev(1, "info", "list_dir"), ev(2, "warn", "slow read", "G:\\clip.ts"), ev(3, "hung", "transfer #4", "reading"), ev(4, "error", "item failed")];
  it("shows problems and hangs without the ordinary lines", () => {
    expect(filterEvents(all, "problems", "").map((e) => e.seq)).toEqual([2, 3, 4]);
    expect(filterEvents(all, "hung", "").map((e) => e.seq)).toEqual([3]);
  });
  it("searches names and details, ignoring case", () => {
    expect(filterEvents(all, "all", "CLIP").map((e) => e.seq)).toEqual([2]);
    expect(filterEvents(all, "problems", "transfer").map((e) => e.seq)).toEqual([3]);
  });
  it("says how long in plain words", () => {
    expect(age(4000)).toBe("4 s");
    expect(age(125_000)).toBe("2 min 5 s");
    expect(age(3_900_000)).toBe("1 h 5 min");
  });
});
