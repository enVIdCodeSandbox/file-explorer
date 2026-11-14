// Whether the phone layout applies, for components and for code outside them.

import { adv } from "../settings/advanced";
import { useStore } from "../store";
import type { Settings } from "../api";
import { isMobileOs } from "./native";

export * from "./native";

/** Whether the phone layout applies: on a phone or tablet, or when it was asked for in Settings. */
export function phoneLayoutFor(settings: Settings | null | undefined, os: string | undefined): boolean {
  const want = settings ? adv<string>(settings, "ui.phoneLayout") : "auto";
  return want === "on" || (want !== "off" && isMobileOs(os));
}

export function useMobile(): boolean {
  return useStore((s) => phoneLayoutFor(s.settings, s.info?.os));
}

/** Not a hook: for code that runs outside a component. */
export const isMobileNow = (): boolean => {
  const s = useStore.getState();
  return phoneLayoutFor(s.settings, s.info?.os);
};
