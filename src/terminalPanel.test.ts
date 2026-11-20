import { describe, expect, it } from "vitest";
import { DEFAULT_LAYOUT, moveTo, resolveLayout } from "./util/layout";

describe("terminal panel in the layout", () => {
  it("is at the bottom by default and can go on any side", () => {
    expect(DEFAULT_LAYOUT.terminal.dock).toBe("bottom");
    for (const dock of ["top", "left", "right", "float", "popout", "hidden"] as const) expect(moveTo(DEFAULT_LAYOUT, "terminal", dock).terminal.dock).toBe(dock);
  });
  it("keeps a dragged size on the same axis and drops it when the axis flips", () => {
    const sized = { ...DEFAULT_LAYOUT, terminal: { ...DEFAULT_LAYOUT.terminal, size: 420 } };
    expect(moveTo(sized, "terminal", "top").terminal.size).toBe(420);
    expect(moveTo(sized, "terminal", "right").terminal.size).toBeUndefined();
  });
  it("reads a saved size, limiting silly ones", () => {
    const saved = (size: unknown) => resolveLayout({ terminal: { dock: "bottom", order: 0, collapsed: false, x: 0, y: 0, w: 500, h: 300, size } });
    expect(saved(350).terminal.size).toBe(350);
    expect(saved(5).terminal.size).toBeUndefined();
    expect(saved(99999).terminal.size).toBe(640);
    expect(saved(20 * 5).terminal.size).toBe(110);
    expect(saved("wide").terminal.size).toBeUndefined();
  });
});
