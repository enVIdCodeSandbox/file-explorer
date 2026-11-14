#!/usr/bin/env node
// Finishes the phone projects that `tauri android init` and `tauri ios init`
// generate. Those projects live in src-tauri/gen (not kept in git, so they
// always match the installed Tauri version); what this app adds to them lives
// in src-tauri/mobile and is applied here. Safe to run more than once.
//
//   node scripts/mobile-setup.mjs android    after  npx tauri android init
//   node scripts/mobile-setup.mjs ios        after  npx tauri ios init   (on a Mac)
//
// Android: storage permissions, the activity that asks for "All files access"
// and opens and shares files, a rule that keeps its methods through code
// shrinking, and release signing from keystore.properties when that file exists.
// iOS: shows the app's folder in the Files app.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tauri = join(root, "src-tauri");
const extras = join(tauri, "mobile");
const identifier = JSON.parse(readFileSync(join(tauri, "tauri.conf.json"), "utf8")).identifier;
// Tauri turns dashes in the identifier into underscores for the Java package.
const pkg = identifier.replace(/-/g, "_");

const say = (text) => console.log(`mobile-setup: ${text}`);
const fail = (text) => {
  console.error(`mobile-setup: ${text}`);
  process.exit(1);
};

/** Change a file's text; reports whether anything changed. */
function edit(file, change) {
  const before = readFileSync(file, "utf8");
  const after = change(before);
  if (after === before) return false;
  writeFileSync(file, after);
  return true;
}

function android() {
  const app = join(tauri, "gen", "android", "app");
  if (!existsSync(app)) fail("src-tauri/gen/android is missing. Run `npx tauri android init` first.");

  // 1. The activity.
  const source = join(app, "src", "main", "java", ...pkg.split("."));
  mkdirSync(source, { recursive: true });
  writeFileSync(join(source, "MainActivity.kt"), readFileSync(join(extras, "android", "MainActivity.kt"), "utf8").replaceAll("__PACKAGE__", pkg));
  writeFileSync(join(app, "proguard-fast.pro"), readFileSync(join(extras, "android", "proguard-fast.pro"), "utf8").replaceAll("__PACKAGE__", pkg));
  say("activity and keep rules written");

  // 2. Permissions, and the older storage behavior on Android 10.
  const manifest = join(app, "src", "main", "AndroidManifest.xml");
  const permissions = readFileSync(join(extras, "android", "permissions.xml"), "utf8").trimEnd();
  edit(manifest, (xml) => {
    if (!xml.includes("MANAGE_EXTERNAL_STORAGE")) {
      const at = xml.indexOf("<application");
      if (at < 0) fail("AndroidManifest.xml has no <application> element.");
      xml = `${xml.slice(0, at)}${permissions.trimStart()}\n\n    ${xml.slice(at)}`;
    }
    if (!xml.includes("requestLegacyExternalStorage")) xml = xml.replace("<application", '<application\n        android:requestLegacyExternalStorage="true"');
    return xml;
  });
  say("permissions added to the manifest");

  // 3. Files anywhere on the device (a memory card, not only built-in storage) can be handed to other apps.
  const paths = join(app, "src", "main", "res", "xml", "file_paths.xml");
  if (existsSync(paths)) edit(paths, (xml) => (xml.includes("<root-path") ? xml : xml.replace("</paths>", '  <root-path name="device" path="." />\n</paths>')));

  // 4. Sign release builds with the key named in gen/android/keystore.properties, when there is one.
  const gradle = join(app, "build.gradle.kts");
  const changed = edit(gradle, (text) => {
    if (text.includes("fastKeystore")) return text;
    if (!text.includes("import java.io.FileInputStream")) text = `import java.io.FileInputStream\n${text}`;
    const signing = `    val fastKeystore = rootProject.file("keystore.properties")
    if (fastKeystore.exists()) {
        signingConfigs {
            create("release") {
                val keys = Properties().apply { load(FileInputStream(fastKeystore)) }
                keyAlias = keys.getProperty("keyAlias")
                keyPassword = keys.getProperty("password")
                storeFile = file(keys.getProperty("storeFile"))
                storePassword = keys.getProperty("password")
            }
        }
    }
`;
    if (!text.includes("    buildTypes {")) fail("build.gradle.kts has no buildTypes block; the Tauri template changed.");
    text = text.replace("    buildTypes {", `${signing}    buildTypes {`);
    const release = 'getByName("release") {';
    if (!text.includes(release)) fail("build.gradle.kts has no release build type; the Tauri template changed.");
    return text.replace(release, `${release}\n            if (fastKeystore.exists()) signingConfig = signingConfigs.getByName("release")`);
  });
  say(changed ? "release signing wired to keystore.properties" : "release signing already wired");
}

function ios() {
  const apple = join(tauri, "gen", "apple");
  if (!existsSync(apple)) fail("src-tauri/gen/apple is missing. Run `npx tauri ios init` first (on a Mac).");
  const target = readdirSync(apple).find((d) => d.endsWith("_iOS") && existsSync(join(apple, d, "Info.plist")));
  if (!target) fail("No *_iOS/Info.plist in src-tauri/gen/apple; the Tauri template changed.");
  const plist = join(apple, target, "Info.plist");
  const additions = JSON.parse(readFileSync(join(extras, "ios", "Info.plist.additions.json"), "utf8"));
  const buddy = (command) => execFileSync("/usr/libexec/PlistBuddy", ["-c", command, plist], { stdio: ["ignore", "pipe", "pipe"] });
  for (const [key, value] of Object.entries(additions)) {
    const kind = typeof value === "boolean" ? "bool" : "string";
    try {
      buddy(`Set :${key} ${value}`);
    } catch {
      buddy(`Add :${key} ${kind} ${value}`);
    }
  }
  say(`Info.plist updated (${Object.keys(additions).join(", ")})`);
}

const which = process.argv[2];
if (which === "android") android();
else if (which === "ios") ios();
else fail("say which project to finish: android or ios");
