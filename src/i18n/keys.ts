// The English texts that have translations, in a fixed order. Each language
// file lists its translations in this same order (see `dict`), so a missing or
// extra line is caught at once instead of shifting everything after it.

export const KEYS = [
  "Welcome", // 1
  "Interface",
  "Colors",
  "Performance",
  "Saving",
  "Privacy",
  "Review",
  "Setup progress",
  "Skip setup",
  "← Back", // 10
  "Next →",
  "Confirm and start",
  "Welcome to FAST File Explorer",
  "A few quick choices to make it yours, one topic at a time. Use Next and Back or the arrow keys. At the end you will see everything you chose before it is saved.",
  "processor threads",
  "memory",
  "battery",
  "Yes",
  "No",
  "The recommendations on the next views are based on this computer.", // 20
  "Language",
  "Automatic (system language)",
  "Recommended",
  "Which file manager are you used to?",
  "This sets the layout, icons and shortcuts together.",
  "Interface style",
  "Like File Explorer in Windows 11",
  "Like Finder on a Mac",
  "Clean and simple, like GNOME or KDE",
  "Light or dark?", // 30
  "You can pick from many more themes later in Settings.",
  "Match my system",
  "Dark",
  "Light",
  "Follows your computer, day and night.",
  "Easy on the eyes in dim rooms.",
  "Bright and crisp in daylight.",
  "How much of this computer should it use?",
  "More means faster search and folders, but more memory and background work.",
  "This computer has {cpus} threads and {ram} of memory.", // 40
  "Old or small devices. Saves memory and battery.",
  "Powerful PCs. Fastest search and folders.",
  "Recommended for this computer",
  "Suits:",
  "Performance level",
  "Heavy",
  "What may it remember?",
  "Remembering makes things faster. It is all saved on this computer only. Turn off anything you do not want kept.",
  "Folder sizes",
  "Show a folder's size at once next time.", // 50
  "Thumbnails",
  "Keep small previews so picture folders open at once.",
  "This computer's details",
  "Keep hardware and drive health so About opens at once.",
  "Folders you open next",
  "Learn your habits to get the next folder ready.",
  "Previous searches",
  "Show recent searches under the search box.",
  "On",
  "Off", // 60
  "Saved only on this computer",
  "Nothing here is ever uploaded.",
  "Everything stays on this computer",
  "All information is stored locally. Nothing gets sent to a cloud.",
  "Your settings, history and saved details never leave this device.",
  "Hide server addresses",
  "Network drives show a friendly name instead of an IP address or host.",
  "Tip: lock a folder or drive later by right-clicking it and choosing “Lock with passcode…”.",
  "Here is what you chose",
  "(recommended)", // 70
  "Layout, icons, colors and shortcuts like {name}",
  "Last {n}",
  "All of this can be changed at any time in Settings, at the top right (or press Ctrl+,).",
] as const;

/** Pair a language's lines with the English texts, in order. */
export function dict(lines: readonly string[]): Record<string, string> {
  if (lines.length !== KEYS.length) throw new Error(`translation has ${lines.length} lines, expected ${KEYS.length}`);
  return Object.fromEntries(KEYS.map((k, i) => [k, lines[i]]));
}
