// The shortcut shown beside a menu item: whatever it is set to now, and nothing when it is switched off or not set.

import { activeKey } from "../settings/keys";
import { useStore } from "../store";

export const sk = (id: string): string => activeKey(useStore.getState().settings, id).replace("ArrowUp", "↑").replace("ArrowDown", "↓").replace("ArrowLeft", "←").replace("ArrowRight", "→");

/** The key of a command with Shift added (Delete, then Shift+Delete). */
export const skShift = (id: string): string => {
  const k = sk(id);
  return k && !k.includes("Shift+") ? (k.includes("+") ? k.replace(/\+(?=[^+]*$)/, "+Shift+") : `Shift+${k}`) : k;
};
