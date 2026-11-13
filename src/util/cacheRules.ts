// Folders and drives where nothing is saved (Settings > Storage). The list
// lives in the advanced settings; the app enforces it in each cache.

import type { Settings } from "../api";

export const RULES_KEY = "cache.exclude";

export function cacheRules(settings: Settings | null | undefined): string[] {
  const v = settings?.advanced?.[RULES_KEY];
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string" && x.trim() !== "") : [];
}
