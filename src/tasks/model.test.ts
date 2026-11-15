import { describe, expect, it } from "vitest";
import { ACTION_TYPES, describeAction, describeEvery, describeTrigger, newAction, newTask, newTrigger, TRIGGER_TYPES, validate } from "./model";

describe("task model", () => {
  it("starts from something that only needs filling in", () => {
    const t = newTask();
    expect(validate(t)).toMatch(/name/);
    t.name = "Backup";
    expect(validate(t)).toMatch(/Step 1/);
    if (t.actions[0].type === "copy") {
      t.actions[0].from = ["/a"];
      t.actions[0].to = "/b";
    }
    expect(validate(t)).toBeNull();
  });

  it("asks for the details each kind of step needs", () => {
    const t = { ...newTask(), name: "x" };
    for (const { type } of ACTION_TYPES) {
      t.actions = [newAction(type)];
      expect(validate(t), type).not.toBeNull();
    }
    t.actions = [];
    expect(validate(t)).toMatch(/at least one step/);
    t.actions = [newAction("run")];
    t.triggers = [];
    expect(validate(t)).toMatch(/when it runs/i);
  });

  it("builds every kind of trigger and describes it in words", () => {
    for (const { type } of TRIGGER_TYPES) expect(describeTrigger(newTrigger(type)).length).toBeGreaterThan(3);
    expect(describeTrigger({ type: "daily", time: "09:00", days: [0, 2] })).toBe("Mon, Wed at 09:00");
    expect(describeTrigger({ type: "daily", time: "09:00", days: [] })).toBe("Every day at 09:00");
    expect(describeEvery(90)).toBe("90 minutes");
    expect(describeEvery(120)).toBe("2 hours");
    expect(describeEvery(1440)).toBe("day");
    expect(describeTrigger({ type: "driveConnected", name: "" })).toMatch(/any drive/);
  });

  it("describes steps by the last part of their paths", () => {
    const a = newAction("copy");
    if (a.type === "copy") {
      a.from = ["C:\\Users\\me\\Photos", "C:\\x\\y.txt"];
      a.to = "D:\\Backup\\";
    }
    expect(describeAction(a)).toBe("Copy Photos and 1 more to Backup");
  });

  it("matches the shape the backend stores", () => {
    const json = JSON.parse(JSON.stringify(newTask()));
    expect(Object.keys(json.options).sort()).toEqual(["catchUp", "lowPriority", "notify", "onSleep", "onlyOnAc", "retries", "retryDelaySecs", "runAsAdmin", "skipIfRunning", "stopOnError", "timeoutMinutes"]);
    expect(json.actions[0].keepStructure).toBe(true);
    expect(Object.keys(json.actions[0].filter).sort()).toEqual(["exclude", "hidden", "include", "maxBytes", "minBytes", "newerThanDays", "olderThanDays", "recursive"]);
  });
});
