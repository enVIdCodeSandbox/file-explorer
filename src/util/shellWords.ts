// Typing the name of a shell in the address bar opens it in the folder being viewed, as Windows Explorer does
// with "cmd" and "powershell".

export interface ShellChoice {
  /** Windows: the terminal program to use; elsewhere left to the settings. */
  shell?: string;
  /** macOS and Linux: the shell to run inside the terminal. */
  loginShell?: string;
  /** What to call it in the confirmation. */
  name: string;
}

const WINDOWS: Record<string, ShellChoice> = {
  cmd: { shell: "cmd", name: "Command Prompt" },
  powershell: { shell: "powershell", name: "PowerShell" },
  pwsh: { shell: "pwsh", name: "PowerShell 7" },
  wt: { shell: "wt", name: "Windows Terminal" },
  wsl: { shell: "wsl", name: "WSL" },
};

const UNIX_SHELLS = ["bash", "zsh", "fish", "sh"];

/** The shell a typed word names, or null when it is just a path (or anything else). */
export function shellFor(typed: string, os: string | undefined): ShellChoice | null {
  const word = typed.trim().toLowerCase().replace(/\.exe$/, "");
  if (!word || /[\\/:~.]/.test(word)) return null;
  if (os === "windows") return WINDOWS[word] ?? (word === "terminal" ? WINDOWS.wt : null);
  if (word === "terminal" || word === "term") return { name: "Terminal" };
  return UNIX_SHELLS.includes(word) ? { loginShell: word, name: word } : null;
}
