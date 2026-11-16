import { describe, expect, it } from "vitest";
import { ancestors } from "./components/Panes";

describe("Miller columns", () => {
  it("lists the folders from the top down", () => {
    expect(ancestors("C:\\Users\\me\\Docs")).toEqual(["C:\\", "C:\\Users", "C:\\Users\\me", "C:\\Users\\me\\Docs"]);
    expect(ancestors("/home/me")).toEqual(["/", "/home", "/home/me"]);
    expect(ancestors("/")).toEqual(["/"]);
  });
});
