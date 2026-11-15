import { describe, expect, it } from "vitest";
import { readViews, withView, withoutColumns } from "./folderViews";
import { NO_FILTER } from "./store";

describe("what is kept about each folder", () => {
  it("keeps parts separately and forgets one when asked", () => {
    let v = withView({}, "/a", { sort: { key: "size", asc: false } });
    v = withView(v, "/a", { filter: { ...NO_FILTER, text: "x" } });
    expect(v["/a"].sort).toEqual({ key: "size", asc: false });
    expect(v["/a"].filter?.text).toBe("x");
    v = withView(v, "/a", { sort: null });
    expect(v["/a"].sort).toBeUndefined();
    v = withView(v, "/a", { filter: null });
    expect(v["/a"]).toBeUndefined();
  });

  it("drops the folders used longest ago past the limit", () => {
    let v = {};
    for (const n of ["/1", "/2", "/3"]) v = withView(v, n, { sort: { key: "name", asc: true } }, 2);
    expect(Object.keys(v)).toEqual(["/2", "/3"]);
    v = withView(v, "/2", { sort: { key: "type", asc: true } }, 2); // used again: now the newest
    v = withView(v, "/4", { sort: { key: "name", asc: true } }, 2);
    expect(Object.keys(v)).toEqual(["/2", "/4"]);
  });

  it("reads saved data defensively", () => {
    expect(readViews(null)).toEqual({});
    expect(readViews("not json")).toEqual({});
    const v = readViews(JSON.stringify({ "/ok": { sort: { key: "size", asc: true }, columns: { order: ["size", "zzz"], widths: {} } }, "/bad": { sort: { key: 3 } }, "/x": 5 }));
    expect(Object.keys(v)).toEqual(["/ok"]);
    expect(v["/ok"].columns?.order).toEqual(["name", "size"]);
  });

  it("forgets every folder's own columns, keeping the rest", () => {
    const v = withoutColumns({ "/a": { columns: { order: ["name"], widths: {} } }, "/b": { sort: { key: "name", asc: true }, columns: { order: ["name"], widths: {} } } });
    expect(v).toEqual({ "/b": { sort: { key: "name", asc: true } } });
  });
});
