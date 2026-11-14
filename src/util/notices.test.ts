import { describe, expect, it } from "vitest";
import { itemFacts } from "./notices";

describe("itemFacts", () => {
  it("lists the count, the shared folder and the names", () => {
    const r = itemFacts("Copy", ["/home/a/x.txt", "/home/a/y.txt"], [["Status", "Ready"]]);
    expect(r.facts).toEqual([["Action", "Copy"], ["Items", "2"], ["Location", "/home/a"], ["Status", "Ready"]]);
    expect(r.names).toEqual(["x.txt", "y.txt"]);
  });
  it("says how many folders when items come from several", () => {
    expect(itemFacts("Delete", ["/a/x", "/b/y"]).facts[2]).toEqual(["Locations", "2 folders"]);
  });
});
