import { describe, it, expect } from "vitest";
import { nextPulse } from "./pulse";

describe("a restartable animation", () => {
  it("alternates names, so each trigger starts the animation again", () => {
    const first = nextPulse(undefined);
    const second = nextPulse(first);
    const third = nextPulse(second);
    expect(first).toBe("a");
    expect(second).toBe("b");
    expect(third).toBe("a");
  });
});
