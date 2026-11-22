import { describe, expect, it } from "vitest";
import type { Progress } from "../api";
import { bulkChoices, pausableTransfers } from "./bulkJobs";

const job = (id: number, phase: string) => ({ id, phase }) as unknown as Progress;
const jobs = { 1: job(1, "running"), 2: job(2, "paused"), 3: job(3, "scanning"), 4: job(4, "queued"), 5: job(5, "running") };

describe("pause all / resume all", () => {
  it("pauses only what is running", () => expect(pausableTransfers(jobs, true).map((p) => p.id)).toEqual([1, 5]));
  it("resumes only what is paused", () => expect(pausableTransfers(jobs, false).map((p) => p.id)).toEqual([2]));
  it("leaves out jobs still counting or waiting in line", () => {
    expect(pausableTransfers({ 3: jobs[3], 4: jobs[4] }, true)).toEqual([]);
    expect(pausableTransfers({ 3: jobs[3], 4: jobs[4] }, false)).toEqual([]);
  });
  it("offers each button only when it would do something", () => {
    expect(bulkChoices(jobs, 5)).toEqual({ pause: 2, resume: 1, stop: 5 });
    expect(bulkChoices({ 2: jobs[2] }, 1)).toEqual({ pause: 0, resume: 1, stop: 1 });
    expect(bulkChoices({}, 0)).toEqual({ pause: 0, resume: 0, stop: 0 });
  });
});
