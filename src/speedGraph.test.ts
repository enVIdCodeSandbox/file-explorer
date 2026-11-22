import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { SpeedGraph } from "./components/Transfers";

const samples = Array.from({ length: 60 }, (_, i) => ({ t: i * 2.5, filesPerSec: i % 10 === 0 ? 3 : 0.04, bytesPerSec: i === 0 ? 2e9 : 1.5e8 }));

describe("speed graph", () => {
  it("ends at the projected finish while running, and at the last moment once finished", () => {
    const live = renderToString(createElement(SpeedGraph, { samples, total: null, etaSecs: 1260 }));
    expect(live).toContain("23 min 28 sec"); // 147.5 s + 1260 s
    const done = renderToString(createElement(SpeedGraph, { samples, total: null, etaSecs: 0, finished: true, ranFor: 150 }));
    expect(done).toContain("2 min 30 sec");
    expect(done).not.toContain("graph-streak");
  });
  it("offers the zoom controls", () => {
    const html = renderToString(createElement(SpeedGraph, { samples, total: null, etaSecs: 30 }));
    for (const t of ["1 s", "10 s", "1 min", "All", "Zoom in", "Zoom out"]) expect(html).toContain(t);
  });
});
