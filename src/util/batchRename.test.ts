import { describe, expect, it } from "vitest";
import { DEFAULT_RULES, orderRenames, planRenames, splitName, type RenameInput, type RenameRules } from "./batchRename";

const file = (name: string, modified = 0): RenameInput => ({ name, isDir: false, modified });
const rules = (patch: Partial<RenameRules>): RenameRules => ({ ...DEFAULT_RULES, ...patch });
const names = (items: RenameInput[], r: Partial<RenameRules>, siblings: string[] = []) => planRenames(items, rules(r), siblings).map((p) => p.to);

describe("splitName", () => {
  it("keeps the last extension, and none for folders and dotfiles", () => {
    expect(splitName("a.tar.gz", false)).toEqual(["a.tar", ".gz"]);
    expect(splitName("v1.2", true)).toEqual(["v1.2", ""]);
    expect(splitName(".bashrc", false)).toEqual([".bashrc", ""]);
    expect(splitName("README", false)).toEqual(["README", ""]);
  });
});

describe("planRenames", () => {
  it("leaves names alone by default", () => {
    const plans = planRenames([file("a.txt")], DEFAULT_RULES);
    expect(plans[0]).toMatchObject({ to: "a.txt", changed: false, problem: null });
  });

  it("finds and replaces without touching the extension", () => {
    expect(names([file("IMG_001.jpg"), file("img_002.JPG")], { find: "img_", replace: "Holiday " })).toEqual(["Holiday 001.jpg", "Holiday 002.JPG"]);
    expect(names([file("IMG_001.jpg")], { find: "img_", replace: "x", matchCase: true })).toEqual(["IMG_001.jpg"]);
    expect(names([file("jpg.jpg")], { find: "jpg", replace: "png" })).toEqual(["png.jpg"]);
  });

  it("treats the search text literally, not as a pattern", () => {
    expect(names([file("a.b (1).txt")], { find: ".b (", replace: "_" })).toEqual(["a_1).txt"]);
  });

  it("numbers items with a start value and padding", () => {
    expect(names([file("x.png"), file("y.png"), file("z.png")], { template: "Photo {n}", start: 8, digits: 3 })).toEqual(["Photo 008.png", "Photo 009.png", "Photo 010.png"]);
  });

  it("puts the original name and the date into the template", () => {
    const t = new Date(2024, 4, 9, 12).getTime() / 1000;
    expect(names([file("a.txt", t)], { template: "{date} {name}" })).toEqual(["2024-05-09 a.txt"]);
  });

  it("changes case", () => {
    expect(names([file("hello world.TXT")], { letters: "upper" })).toEqual(["HELLO WORLD.TXT"]);
    expect(names([file("Hello World.txt")], { letters: "lower" })).toEqual(["hello world.txt"]);
    expect(names([file("the big_lebowski-1999.mkv")], { letters: "title" })).toEqual(["The Big_Lebowski-1999.mkv"]);
  });

  it("does not add an extension to folders", () => {
    const plans = planRenames([{ name: "v1.2", isDir: true, modified: 0 }], rules({ template: "{name} old" }));
    expect(plans[0].to).toBe("v1.2 old");
  });

  it("reports empty, invalid, too long and clashing names", () => {
    expect(planRenames([file("a.txt")], rules({ template: " " }))[0].problem).toBe("empty");
    expect(planRenames([file("a.txt")], rules({ template: "a/b" }))[0].problem).toBe("invalid");
    expect(planRenames([file("a.txt")], rules({ template: "con" }))[0].problem).toBe("invalid");
    expect(planRenames([file("a.txt")], rules({ template: "x".repeat(300) }))[0].problem).toBe("tooLong");
    expect(planRenames([file("a.txt")], rules({ template: "b" }), ["a.txt", "B.txt"])[0].problem).toBe("exists");
    const same = planRenames([file("a.txt"), file("b.txt")], rules({ template: "same" }));
    expect(same.map((p) => p.problem)).toEqual(["duplicate", "duplicate"]);
  });

  it("does not count the selected items themselves as clashes", () => {
    expect(planRenames([file("a.txt"), file("b.txt")], rules({ template: "{n}" }), ["a.txt", "b.txt", "z.txt"]).map((p) => p.problem)).toEqual([null, null]);
  });

  it("drops trailing dots and spaces that Windows would remove", () => {
    expect(names([file("a.txt")], { template: "name. " })).toEqual(["name.txt"]);
  });
});

/** Run the rounds on a pretend folder and return the names left in it. */
function run(rounds: { from: string; to: string }[][], start: string[]): string[] {
  const fs = new Set(start.map((s) => s.toLowerCase()));
  const order = [...start];
  for (const round of rounds) {
    for (const { from, to } of round) {
      expect(fs.has(from.toLowerCase()), `${from} exists`).toBe(true);
      expect(fs.has(to.toLowerCase()) && to.toLowerCase() !== from.toLowerCase(), `${to} is free`).toBe(false);
      fs.delete(from.toLowerCase());
      fs.add(to.toLowerCase());
      order[order.findIndex((n) => n.toLowerCase() === from.toLowerCase())] = to;
    }
  }
  return order;
}

describe("orderRenames", () => {
  it("is a single round when nothing is in the way", () => {
    const plans = planRenames([file("a"), file("b")], rules({ template: "{name}2" }));
    expect(orderRenames(plans)).toHaveLength(1);
  });

  it("does nothing when nothing changes", () => {
    expect(orderRenames(planRenames([file("a")], DEFAULT_RULES))).toEqual([]);
  });

  it("shifts a numbered series up by one without overwriting", () => {
    const items = [file("1.txt"), file("2.txt"), file("3.txt")];
    const plans = planRenames(items, rules({ template: "{n}", start: 2 }), ["1.txt", "2.txt", "3.txt"]);
    expect(plans.map((p) => p.problem)).toEqual([null, null, null]);
    expect(run(orderRenames(plans, "t"), ["1.txt", "2.txt", "3.txt"])).toEqual(["2.txt", "3.txt", "4.txt"]);
  });

  it("swaps two names", () => {
    const plans = [
      { from: "a", to: "b", changed: true, problem: null },
      { from: "b", to: "a", changed: true, problem: null },
    ];
    expect(run(orderRenames(plans, "t"), ["a", "b"])).toEqual(["b", "a"]);
  });

  it("renames a name that differs only by case", () => {
    const plans = planRenames([file("readme.md")], rules({ letters: "upper" }));
    expect(run(orderRenames(plans, "t"), ["readme.md"])).toEqual(["README.md"]);
  });

  it("skips items with problems", () => {
    const plans = planRenames([file("a.txt"), file("b.txt")], rules({ template: "same" }));
    expect(orderRenames(plans)).toEqual([]);
  });
});
