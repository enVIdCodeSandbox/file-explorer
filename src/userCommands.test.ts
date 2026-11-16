import { describe, expect, it } from "vitest";
import { buildRun, expandPlaceholders, insertAtCaret, splitWords } from "./util/userCommands";

const ctx = { folder: "C:\\Users\\me\\My Docs", cursor: { name: "report final.txt", isDir: false }, selected: ["a.txt", "b c.txt"] };

describe("user command placeholders", () => {
  it("expands the folder and the item under the cursor", () => {
    expect(expandPlaceholders("%P", ctx)).toBe('"C:\\Users\\me\\My Docs\\"');
    expect(expandPlaceholders("%p", { ...ctx, folder: "/home/me/x" })).toBe("/home/me/x");
    expect(expandPlaceholders("%N", ctx)).toBe('"report final.txt"');
    expect(expandPlaceholders("%n.%E", ctx)).toBe('"report final".txt');
    expect(expandPlaceholders("%F", ctx)).toBe('"C:\\Users\\me\\My Docs\\report final.txt"');
    expect(expandPlaceholders("%D", ctx)).toBe('"My Docs"');
  });
  it("does not add quotes inside quotes", () => {
    expect(expandPlaceholders('"%P%N"', ctx)).toBe('"C:\\Users\\me\\My Docs\\report final.txt"');
  });
  it("gives the selection, or the cursor item when nothing is selected", () => {
    expect(expandPlaceholders("%S", ctx)).toBe('"a.txt" "b c.txt"');
    expect(expandPlaceholders("%L", { ...ctx, folder: "/x", selected: ["a", "b"] })).toBe('"/x/a" "/x/b"');
    expect(expandPlaceholders("%S", { ...ctx, selected: [] })).toBe('"report final.txt"');
  });
  it("keeps unknown placeholders and %%", () => {
    expect(expandPlaceholders("100%% %z 5%", ctx)).toBe("100% %z 5%");
  });
  it("splits words and builds a run", () => {
    expect(splitWords('a "b c" \'d e\' ""')).toEqual(["a", "b c", "d e", ""]);
    const r = buildRun({ program: "code", params: "-g %F --folder %p", startDir: "" }, ctx);
    expect(r.args).toEqual(["-g", "C:\\Users\\me\\My Docs\\report final.txt", "--folder", "C:\\Users\\me\\My Docs"]);
    expect(r.cwd).toBe(ctx.folder);
  });
  it("inserts a name at the caret for the command line", () => {
    expect(insertAtCaret("cat", 3, "my file.txt")).toEqual({ text: 'cat "my file.txt" ', caret: 18 });
    expect(insertAtCaret("", 0, "a.txt").text).toBe("a.txt ");
  });
});
