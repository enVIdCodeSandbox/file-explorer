import { describe, expect, it } from "vitest";
import { adv } from "../settings/advanced";

describe("Esc in the Open with window", () => {
  it("closes it straight away by default, and can be switched off in the advanced settings", () => {
    expect(adv<boolean>(null, "files.openWithEsc")).toBe(true);
    expect(adv<boolean>({ advanced: { "files.openWithEsc": false } } as never, "files.openWithEsc")).toBe(false);
  });
});
