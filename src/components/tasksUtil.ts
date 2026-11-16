// Small helpers the explorer uses to talk to the tasks window without loading it.

import { useEffect, useRef, useState } from "react";
import { api, on } from "../api";
import { newAction, newTask, type Task } from "../tasks/model";

/** A task already filled in from files that were chosen in the explorer ("Automate…"). */
export function taskFromSelection(paths: string[], folder: string | null): Task {
  const t = newTask();
  t.name = paths.length === 1 ? `Back up ${paths[0].split(/[\\/]/).pop()}` : folder ? `Back up ${folder.split(/[\\/]/).pop() || folder}` : "New task";
  const copy = newAction("copy");
  if (copy.type === "copy") copy.from = paths;
  t.actions = [copy];
  return t;
}

export function useTasksRunning(): number {
  const [n, setN] = useState(0);
  const ref = useRef(0);
  useEffect(() => {
    const load = () => void api.tasksList().then((l) => ((ref.current = l.running.length), setN(l.running.length))).catch(() => {});
    load();
    const offs = [on("task-started", load), on("task-finished", load), on("tasks-changed", load)];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, []);
  return n;
}
