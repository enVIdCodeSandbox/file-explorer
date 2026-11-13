// Choose a file from the file view itself. A form that needs a local file
// (an SSH key, say) steps aside, the window shows the folder the file is
// likely in, and the person either browses to the file and opens it, selects it
// and presses Choose, or types its full path. Then the tab goes back to where
// it was and the form returns with the path filled in.

import { api } from "../api";
import { useStore } from "../store";
import { joinPath } from "./path";

export async function pickFile(title: string, startIn?: string, kind: "file" | "folder" = "file"): Promise<string | null> {
  const st = useStore.getState();
  if (st.picker) return null;
  const tab = st.tabs.find((t) => t.id === st.activeTab);
  const back = tab?.location;
  const home = st.info?.home;
  // Start where the file most likely is, and fall back to the home folder.
  let start: string | undefined;
  for (const candidate of [startIn, home]) {
    if (!candidate) continue;
    try {
      await api.listDir(candidate);
      start = candidate;
      break;
    } catch {
      /* try the next place */
    }
  }
  return new Promise<string | null>((resolve) => {
    useStore.setState({
      picker: {
        title,
        kind,
        done: (path) => {
          useStore.setState({ picker: null });
          if (back) useStore.getState().navigate(back);
          resolve(path);
        },
      },
    });
    if (start) useStore.getState().navigate(start);
  });
}

/** Choose a folder or a drive the same way: browse to it and press Choose, or type its path. */
export function pickFolder(title: string, startIn?: string): Promise<string | null> {
  return pickFile(title, startIn, "folder");
}

/** `~/.ssh` for the current person, joined the way this system joins paths. */
export function sshFolder(home: string | null | undefined): string | undefined {
  return home ? joinPath(home, ".ssh") : undefined;
}
