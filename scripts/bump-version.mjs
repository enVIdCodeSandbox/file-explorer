#!/usr/bin/env node
// Raises the version everywhere it is written, in one go.
//   node scripts/bump-version.mjs patch     small fixes and tweaks      0.2.0 -> 0.2.1
//   node scripts/bump-version.mjs minor     a new feature or several    0.2.1 -> 0.3.0
//   node scripts/bump-version.mjs major     a big change or a rewrite   0.3.0 -> 1.0.0
//   node scripts/bump-version.mjs 0.4.2     an exact version
//   node scripts/bump-version.mjs           show the current version
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const file = (p) => join(root, p);
const read = (p) => readFileSync(file(p), "utf8");
const write = (p, text) => writeFileSync(file(p), text);

const current = /^version = "(\d+\.\d+\.\d+)"/m.exec(read("Cargo.toml"))?.[1];
if (!current) throw new Error("no version found in Cargo.toml");
const arg = process.argv[2];
if (!arg) {
  console.log(current);
  process.exit(0);
}
const [maj, min, pat] = current.split(".").map(Number);
const next = { patch: `${maj}.${min}.${pat + 1}`, minor: `${maj}.${min + 1}.0`, major: `${maj + 1}.0.0` }[arg] ?? arg;
if (!/^\d+\.\d+\.\d+$/.test(next)) throw new Error(`"${arg}" is not patch, minor, major or a version like 1.2.3`);

// Cargo.toml: the workspace version every crate uses.
write("Cargo.toml", read("Cargo.toml").replace(/^version = "\d+\.\d+\.\d+"/m, `version = "${next}"`));
// tauri.conf.json and package.json: first "version" key.
for (const p of ["src-tauri/tauri.conf.json", "package.json"]) {
  write(p, read(p).replace(/("version":\s*")\d+\.\d+\.\d+(")/, `$1${next}$2`));
}
// package-lock.json: the project's own entries (the first two "version" keys that follow its name).
if (existsSync(file("package-lock.json"))) {
  let text = read("package-lock.json");
  text = text.replace(/("name": "fast-file-explorer",\s*"version": ")\d+\.\d+\.\d+(")/g, `$1${next}$2`);
  write("package-lock.json", text);
}
// Cargo.lock: our own crates.
if (existsSync(file("Cargo.lock"))) {
  let text = read("Cargo.lock");
  for (const name of ["fast-core", "fast-file-explorer"]) {
    text = text.replace(new RegExp(`(name = "${name}"\\nversion = ")\\d+\\.\\d+\\.\\d+(")`), `$1${next}$2`);
  }
  write("Cargo.lock", text);
}
console.log(`${current} -> ${next}`);
