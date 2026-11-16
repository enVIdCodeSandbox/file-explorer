import { describe, expect, it } from "vitest";
import { wheelSteps, zoomStep } from "./zoomKeys";

const k = (key: string, code = "", extra: object = {}) => ({ key, code, ctrlKey: true, metaKey: false, altKey: false, ...extra });

describe("zoom keys", () => {
  it("Ctrl + and Ctrl = make things bigger, from the main or number pad", () => {
    expect(zoomStep(k("+", "Equal"))).toBe(1);
    expect(zoomStep(k("=", "Equal"))).toBe(1);
    expect(zoomStep(k("Add", "NumpadAdd"))).toBe(1);
  });
  it("Ctrl - makes them smaller and Ctrl 0 resets", () => {
    expect(zoomStep(k("-", "Minus"))).toBe(-1);
    expect(zoomStep(k("Subtract", "NumpadSubtract"))).toBe(-1);
    expect(zoomStep(k("0", "Digit0"))).toBe(0);
  });
  it("ignores keys without Ctrl, with Alt, or that are something else", () => {
    expect(zoomStep(k("+", "Equal", { ctrlKey: false }))).toBeNull();
    expect(zoomStep(k("+", "Equal", { altKey: true }))).toBeNull();
    expect(zoomStep(k("a", "KeyA"))).toBeNull();
  });
  it("turns wheel movement into steps and keeps the remainder", () => {
    expect(wheelSteps(0, -120)).toEqual({ steps: 2, carry: 0 });
    expect(wheelSteps(0, 60)).toEqual({ steps: -1, carry: 0 });
    let c = 0;
    let steps = 0;
    for (let i = 0; i < 6; i++) {
      const r = wheelSteps(c, -10);
      c = r.carry;
      steps += r.steps;
    }
    expect(steps).toBe(1);
  });
});
