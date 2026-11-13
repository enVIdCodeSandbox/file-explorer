import { describe, expect, it } from "vitest";
import rustSource from "../../crates/fast-core/src/thumbs.rs?raw";
import { DOCUMENT_EXTS, IMAGE_EXTS, VIDEO_EXTS, thumbKind } from "./thumbKinds";

function rustList(name: string): string[] {
  const src = rustSource;
  const m = new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\]`).exec(src);
  if (!m) throw new Error(`${name} not found in thumbs.rs`);
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}

describe("thumbnail kinds", () => {
  it("match the lists the backend uses", () => {
    expect(IMAGE_EXTS).toEqual(rustList("IMAGES"));
    expect(DOCUMENT_EXTS).toEqual(rustList("DOCUMENTS"));
    expect(VIDEO_EXTS).toEqual(rustList("VIDEOS"));
  });
  it("tells kinds apart regardless of case", () => {
    expect(thumbKind("JPG")).toBe("images");
    expect(thumbKind("Pdf")).toBe("documents");
    expect(thumbKind("mkv")).toBe("videos");
    expect(thumbKind("txt")).toBeNull();
  });
});
