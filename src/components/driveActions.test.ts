import { describe, expect, it } from "vitest";
import { explainNetError, needsSignIn } from "./driveActions";

describe("mapping a network drive", () => {
  it("asks to sign in when Windows says the sign-in was cancelled or refused", () => {
    expect(needsSignIn("System error 1223 has occurred.\n\nThe operation was canceled by the user.")).toBe(true);
    expect(needsSignIn("System error 1326 has occurred.\n\nThe user name or password is incorrect.")).toBe(true);
    expect(needsSignIn("System error 5 has occurred.\n\nAccess is denied.")).toBe(true);
    expect(needsSignIn("System error 86 has occurred.")).toBe(true);
  });
  it("does not ask when a name and password cannot help", () => {
    expect(needsSignIn("System error 53 has occurred.\n\nThe network path was not found.")).toBe(false);
    expect(needsSignIn("System error 1219 has occurred.")).toBe(false);
    expect(needsSignIn("System error 53 has occurred. 15 or so")).toBe(false);
  });
  it("explains the errors that have a plain answer", () => {
    expect(explainNetError("System error 1219 has occurred.")).toMatch(/another user name/);
    expect(explainNetError("System error 53 has occurred.")).toMatch(/could not find/);
    expect(explainNetError("something else")).toBe("something else");
  });
});
