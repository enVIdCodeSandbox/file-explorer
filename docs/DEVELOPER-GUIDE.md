# Developer guide

## Set up

Prerequisites: Rust (stable), Node.js 20+, and the Tauri 2 system dependencies for your OS (WebView2 on Windows; Xcode command line tools on macOS; `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libxdo-dev` on Debian/Ubuntu).

```sh
npm install
npm run app:dev        # tauri dev: hot reload for the interface, rebuild on Rust changes
npm run dev            # interface only, in a browser (no Rust: commands will fail)
npm run app:build      # installers for this OS under target/release/bundle
npx tauri build --no-bundle   # just the binary, much faster
```

## Checks (run all before pushing)

```sh
cargo test --workspace            # engine + shell tests
npm test                          # vitest: formatting, drop rules, settings, treemap
npx tsc --noEmit                  # strict type check (npm run build does this too)
npm run build                     # type check + production bundle
# Windows-only code, from any OS (needs: rustup target add x86_64-pc-windows-msvc)
cargo check -p fast-core --target x86_64-pc-windows-msvc
```

CI (`.github/workflows/build.yml`) runs tests on Linux, Windows and macOS and builds installers for each; Linux gates the build.

## Repository map

```
crates/fast-core/src      engine, no UI dependency (see ARCHITECTURE.md for each module)
  transfer/               engine.rs, strategy.rs, eta.rs, probe.rs, method.rs  (TRANSFERS.md)
  history.rs              what past transfers did; picks methods and primes estimates
  share.rs                temporary download links
src-tauri/src             lib.rs (setup, registration), state.rs, commands.rs (IPC),
                          native_dnd.rs (system clipboard + drag-out)
src/                      React interface (INTERFACE.md)
  api.ts                  typed wrappers for every command and event
  store.ts                zustand store: tabs, pins, settings, transfers, clipboard
  dnd.ts                  pointer drag-and-drop manager and drop rules
  about.ts                developer name, links and project text on the About page
  docs/                   Documentation pages (the files in /docs are imported here)
docs/                     the documents you are reading; also shown in Settings → Documentation
```

## Principles

1. **The engine never depends on the UI**, and the UI never touches the disk. Anything that reads or writes files goes through a command that runs on a blocking thread.
2. **Every OS gets an implementation.** OS-specific code lives in `platform.rs` (or a small `#[cfg]` module beside the feature) with a version for Windows, macOS and Linux, and a stub for Android and iOS. Prefer the OS's own facility (Recycle Bin, Trash, `GetDriveTypeW`, `/proc/self/mounts`, `mount`) over guessing.
3. **Answer from memory, verify after.** Show cached data at once; correct it when the real answer arrives.
4. **Learn from what actually happened** rather than hard-coding which tool or setting is "faster" (see `history.rs`).
5. **Be honest in the interface.** Rough estimates say so; features that depend on installed tools or a network route say when they are unavailable.

## How to add…

**A setting.** Field in `crates/fast-core/src/settings.rs` (with a default), the type in `src/api.ts`, an entry in `src/settings/schema.ts`. Unknown or missing fields in old settings files fall back to defaults, so no migration is needed for additions.

**A command.** Write the function in `src-tauri/src/commands.rs` with `#[tauri::command]`, run blocking work with `spawn_blocking`, register it in `lib.rs`'s `generate_handler!`, add a typed wrapper in `api.ts`. Then regenerate `docs/IPC.md` (the table is produced from `commands.rs`).

**An event.** `app.emit(...)` or `emit_to(label, ...)` in Rust; `listen()` in `api.ts`. Add it to the table at the end of `docs/IPC.md`.

**A right-click entry.** `buildMenu` in `FileView.tsx`, and `MENU_CATALOG` in `ContextMenu.tsx` so it can be hidden.

**A keyboard shortcut.** `KEY_COMMANDS` in `src/settings/keys.ts`, handled where `commandFor` is used in `App.tsx`.

**A copy method** (Robocopy, rsync, rclone, SMB, NFS are the existing ones). See "Adding a method" in TRANSFERS.md.

**A drag source or drop target.** Give the element `onPointerDown={(e) => beginDrag(e, payload)}` for a source, or `data-drop="folder" data-drop-path="…"` (or another kind from INTERFACE.md) for a target. Rules for what a drop does belong in `resolveFolderDrop`, with a test.

**A documentation page.** Add a Markdown file in `/docs` and list it in `src/docs/pages.ts`. It appears in Settings → Documentation after the next build; search covers its full text.

**A theme.** See THEMING.md.

## Conventions

* Rust: `cargo fmt`, no warnings in `fast-core`; errors are `CoreError` with a message a person can read. Use `?` and never `unwrap()` on I/O.
* TypeScript: strict mode; no `any`. State that must survive reloads goes through the session or settings, not module variables.
* Comments say why. The function name already says what.
* Tests: pure logic gets unit tests next to it (`#[cfg(test)]`, `*.test.ts`). Transfer behaviour gets an end-to-end test in `engine.rs` using temp directories.
* Paths are `String` at the IPC boundary and `PathBuf` inside Rust. Never join paths with `/` by hand in the interface; use `src/util/path.ts`.

## Testing what unit tests cannot

Some behaviour needs a window and a pointer (drag and drop, tabs, the transfer card). To run the built app headless on Linux:

```sh
xvfb-run -s "-screen 0 1400x900x24" env HOME=$(mktemp -d) target/release/fast-file-explorer &
xdotool search --name "FAST" ; xdotool mousemove … mousedown 1 … mouseup 1 ; import -window root shot.png
```

Use a fresh `HOME` so settings and caches from earlier runs do not leak in.

Not covered by automated tests, and worth checking by hand on the real OS after changing them: Robocopy and `/COMPRESS` (Windows), the native clipboard and drag-out, the cross-window tab drop, the share dialog and firewall prompt, the administrator mapped-drive fallback, anything macOS.

## Releasing

Tag `vX.Y.Z` and push: the workflow builds installers for Windows, macOS (universal) and Linux and drafts a GitHub release. Bump the version in `Cargo.toml` (workspace), `src-tauri/tauri.conf.json` and `package.json` first.

## License

GNU General Public License v3 or later (see `LICENSE`).
