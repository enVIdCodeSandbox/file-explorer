// Who made the app. Edit this file to change what the About page says about
// the developer; nothing else needs to change. Leave a field empty to hide it.

export interface Developer {
  name: string;
  role: string;
  bio: string;
  links: { label: string; url: string }[];
}

export const DEVELOPER: Developer = {
  name: "Erick Tran - enVId Tech",
  role: "Developer, Engineer, Gamer",
  bio: "",
  links: [
    {
      label: "GitHub",
      url: "https://github.com/enVId-tech"
    },
    {
      label: "My website!",
      url: "https://etran.dev/"
    }
  ],
};

export const PROJECT = {
  name: "FAST File Explorer",
  tagline: "A fast, cross-platform file explorer with a smart transfer engine.",
  licenseName: "GNU General Public License v3 or later",
  built: [
    ["Engine", "Rust: listing, indexing, search, disk usage and the transfer engine"],
    ["Shell", "Tauri 2: native windows and the operating system's own web view"],
    ["Interface", "React and TypeScript"],
  ] as [string, string][],
};
