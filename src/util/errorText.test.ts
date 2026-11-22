import { describe, expect, it } from "vitest";
import { looksLikeError } from "./errorText";

describe("which pop-ups are about a problem", () => {
  it("recognizes failures", () => {
    for (const m of [
      "System error 1223 has occurred. The operation was canceled by the user.",
      "U:\\x\\front: The process cannot access the file because it is being used by another process. (os error 32)",
      "Could not rename the file",
      "That name or password was not accepted.",
      "Copy failed",
      "The drive did not answer, so this was skipped.",
    ]) expect(looksLikeError(m), m).toBe(true);
  });
  it("leaves ordinary confirmations alone", () => {
    for (const m of ["Connected.", "Disconnected.", "Copied the path", "Pinned to the sidebar", "It is safe to remove the drive."]) expect(looksLikeError(m), m).toBe(false);
  });
  it("handles nothing", () => {
    expect(looksLikeError(null)).toBe(false);
    expect(looksLikeError("")).toBe(false);
  });
});
