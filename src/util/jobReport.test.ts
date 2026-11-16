import { describe, expect, it } from "vitest";
import type { DeleteProgress, Progress } from "../api";
import { reportFiles, reportHtml, reportJson, reportOf, reportText } from "./jobReport";

const copy = { id: 1, phase: "done", destDir: "D:\\Backup <1>", filesDone: 10, filesTotal: 10, bytesDone: 1e7, bytesTotal: 1e7, elapsedSecs: 65, avgBytesPerSec: 2e6, avgFilesPerSec: 2, peakBytesPerSec: 4e6, peakFilesPerSec: 5, skipped: 1, errorCount: 0, method: "native" } as unknown as Progress;
const samples = [{ t: 0, filesPerSec: 0, bytesPerSec: 0 }, { t: 5, filesPerSec: 3, bytesPerSec: 3e6 }, { t: 10, filesPerSec: 2, bytesPerSec: 2e6 }];
const at = new Date(2026, 10, 16, 2, 50, 0).getTime();

describe("a job saved as a file", () => {
  it("says what happened, when, and how fast", () => {
    const r = reportOf({ kind: "transfer", p: copy, at }, samples, "move");
    const f = Object.fromEntries(r.facts);
    expect(r.title).toBe("Move to D:\\Backup <1>");
    expect(f.Result).toBe("Completed");
    expect(f.Finished).toBe("2026-11-16 02:50:00");
    expect(f.Started).toBe("2026-11-16 02:48:55");
    expect(f.Skipped).toBe("1");
  });

  it("covers deletes too", () => {
    const d = { id: 2, phase: "done", action: "delete", permanent: true, selected: 3, firstName: "a.txt", itemsDone: 3, itemsTotal: 3, bytesDone: 0, bytesTotal: 0, elapsedSecs: 1, errorCount: 0 } as unknown as DeleteProgress;
    const r = reportOf({ kind: "delete", p: d, at }, []);
    expect(r.title).toBe("Delete: a.txt and 2 more");
    expect(Object.fromEntries(r.facts).Operation).toBe("Delete (permanent)");
  });

  it("writes text with the speed table, JSON with numbers, and a page that escapes names", () => {
    const r = reportOf({ kind: "transfer", p: copy, at }, samples);
    expect(reportText(r)).toContain("Speed over time");
    expect(JSON.parse(reportJson(r)).speed).toHaveLength(3);
    const html = reportHtml(r);
    expect(html).toContain("<polyline");
    expect(html).toContain("D:\\Backup &lt;1&gt;");
    expect(html).not.toContain("<1>");
  });

  it("offers the page first, then text and data", () => {
    expect(reportFiles(reportOf({ kind: "transfer", p: copy, at }, [])).map((x) => x.ext)).toEqual(["html", "txt", "json"]);
  });
});
