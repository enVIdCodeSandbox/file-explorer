# Transfers

How a copy, move or delete actually runs, from the click to the last byte. Code: `crates/fast-core/src/transfer/` and `history.rs`; shell glue in `src-tauri/src/commands.rs` (`transfer_start`, `transfer_pause`, `transfer_cancel`, `transfer_force_stop`).

## Life of a job

1. **Spec.** The interface sends a `TransferSpec`: operation (copy / move), sources, destination, conflict policy (ask, skip, overwrite, keep both), whether to keep modified times, large-file threshold, a parallelism cap and the chosen copy method.
2. **Scan.** Sources are walked to count files and bytes. Progress shows "Preparing…" meanwhile.
3. **Plan.** `strategy::choose` picks a plan from the job's shape and the devices involved (see ARCHITECTURE.md). A move inside one volume is a rename; a copy inside one volume tries a copy-on-write clone first.
4. **Method.** If the job is a plain copy that an external tool could do identically, the method is chosen (below). Otherwise the built-in engine runs it.
5. **Estimate.** Prior knowledge (history, then a speed probe) gives the first time estimate; live measurement takes over.
6. **Run.** Worker lanes copy; a ticker thread reports `Progress` events at the progress interval.
7. **Finish.** The result is recorded in the history so later jobs choose better.

## Conflicts (files that already exist)

`Conflict` is `ask`, `skip`, `overwrite` or `keepBoth`. The last three are applied silently. With `ask`:

1. Top-level items are not decided during the scan; every clash is decided in `process()`, one file at a time, so files inside merged folders are included.
2. `Shared::resolve` takes the single `ask_gate`, so only one question is open at a time, fills `Asker.pending` (name, paths, sizes, modified times, question number) and waits. Other workers keep copying files that do not clash.
3. The ticker puts `pending` in `Progress.conflict`. The interface (`askAboutConflict` in `store.ts`) shows the dialog in the window that started the job and answers with `transfer_resolve(id, "skip" | "replace" | "keepBoth", all)`.
4. With `all`, the answer is stored in `Asker.sticky` and used for every later clash without asking.
5. Cancelling while a question is open ends the wait (`resolve` returns `None`, the file is left alone).

Jobs with `ask` always run on the built-in engine (tools cannot ask). The elevated helper has no window, so `elevated_start` turns `ask` into `skip`.

## Copy methods

| Method | Program | Used when | Notes |
|---|---|---|---|
| Built-in | none (this app) | always possible | The only method that does moves, renames, clones, "keep both" and rename-on-arrival |
| Robocopy | `robocopy` | Windows | `/E /COPY:DAT /DCOPY:T /MT:n`, `/XC /XN /XO` for "skip existing" |
| rsync | `rsync` | macOS / Linux, if installed | `-rlDt`, `--ignore-existing` for skip, progress from `--progress` and `--out-format=%l\t%n` |
| SMB-tuned | Windows: `robocopy`; elsewhere: `rsync` | the route touches an SMB share (UNC path, mapped drive, cifs/smbfs mount) | Windows adds `/COMPRESS` (SMB compression; needs a Robocopy that supports it, Windows 11 / Server 2022+). Elsewhere rsync runs with `--whole-file` |
| NFS-tuned | `rsync` | the route touches an NFS mount (nfs/nfs4) | `--whole-file`: rsync's delta algorithm would otherwise read the far copy back over the network before sending |
| rclone | `rclone` | installed, any route | `rclone copy` (folders) / `copyto` (files), `--transfers n --checkers n`, JSON statistics on stderr once a second |

**SMB and NFS are protocols the operating system speaks**, not programs. Nothing in this app implements them. What the app does is recognise a route that crosses an SMB or NFS mount (`method::NetMap` reads `/proc/self/mounts` on Linux, `mount` on macOS, `GetDriveTypeW` and the `\\server` prefix on Windows) and offer the copy settings that suit it as extra candidates.

A tool is only eligible when it can reproduce the built-in engine exactly: copy only, conflict policy not "keep both", no rename-on-arrival, names unchanged (`method::can_run`). Otherwise the built-in engine runs the job, whatever was asked for.

If the person forces a method that is missing or does not fit the route, the job runs on the built-in engine.

### Pause, resume, cancel

Pausing kills the tool; resuming starts it again, and every tool skips what is already there (rsync and rclone by comparing, Robocopy by `/XC /XN /XO`), so work continues where it stopped. Cancel kills it for good. Force stop (below) does not wait for the tool at all.

### Progress parsing

`method::parse` turns one line of tool output into an `Event`:

* `Done(size)`: a file finished (Robocopy lines for copied files; rsync `%l\t%n` lines for non-folders)
* `Partial(n)`: bytes of the current file so far (rsync's counter, carriage-return separated)
* `Total { bytes, files }`: running totals for the whole run (rclone's `stats` object)

`run_tool` converts these into increments on the shared counters. Totals restart from zero on each run of the tool, so a resume after a pause is handled by tracking what was already credited.

## Choosing a method (`History::choose_method`)

Every finished job is stored with its route (source and destination volume), file count, byte count, strategy, method, duration and error count.

For a new job with candidates `[Native, …]`:

1. Fewer than two candidates, a move, or a job under **64 MiB** → built-in. A tool costs more to start than it can save.
2. Look at past jobs on a similar route and size (distance below 2.2). Weight each by how alike it is and by age (half-life 45 days). Score each method by speed: bytes/s when files average 1 MiB or more, files/s otherwise.
3. A method needs **2** such results to be trusted. If the built-in engine has no score yet, built-in runs.
4. With "Try other methods on big jobs" on, and the job at least **256 MiB**, a candidate with too few results is tried once so it can be measured.
5. Otherwise the fastest wins, but an alternative must beat the built-in engine by **10%**, so noise does not flip the choice back and forth.

History file format version is `FORMAT_VERSION` in `history.rs`; method codes are stable (`Method::code`: 0 built-in, 1 Robocopy, 2 rsync, 3 rclone, 4 SMB-tuned, 5 NFS-tuned). Never renumber them.

## Time remaining

* **Model.** `time = a + b × size` per file, fitted while the job runs with exponentially weighted least squares.
* **Head start.** A tracker (`EtaTracker::with_head_start`) begins with a prior instead of nothing:
  * history on the same route with at least 3 jobs: confidence 0.8; fewer: 0.55; other routes: 0.3
  * otherwise a **speed probe** (`probe.rs`): about 8 MiB read, 8 MiB written in chunks of 1 MiB with a sync, and 24 tiny files, at most 1.5 s, in a hidden `.fast-probe-*` folder that is removed afterwards. It runs only when the job has at least 48 MiB or 300 files and no good history; confidence 0.45
  * with neither, default per-device costs
* **Confidence.** The readout shows "Rough estimate", "Getting more accurate" and a brief "Estimate settled". Live data takes over once there are at least 3 samples spanning 0.8 s; the confidence window is 5 s.

## Progress rate

`transfer.progressIntervalMs` (Settings → File transfers). 0 means automatic from the resource budget: 100 ms on full or manual budgets, 250 ms balanced, 500 ms power saver (laptops on battery, weak machines). Manual values are clamped to 50–2000 ms. Faster updates cost interface work, never transfer speed.

## The chart

`SpeedGraph` in `Transfers.tsx` draws bytes/s and files/s over time. The time axis leaves room up to the expected finish. A dashed horizontal line with a dot runs from the newest sample to the right edge at the current speed of each series, so the current rate can be read at a glance even when the full history is long.

## Force stop

Cancelling asks the workers to stop and waits for them to reach a checkpoint, which a hung network call may never do. After Cancel the card shows **Force stop** (Shift-clicking ✕ forces at once). The interface then ignores every later event for that job (`forcedJobs`) and `transfer_force_stop` cancels the job, drops its handle and, for elevated helper jobs, writes the cancel file. A blocked thread may linger until the OS call returns; it can no longer affect the interface or touch more files.

## Deleting

`delete_start(paths, permanent)`. Without `permanent`, items go to the Recycle Bin / Trash through `platform.rs`. As administrator, items cannot be recycled and are deleted permanently; the elevation prompt says so. Shift-click on the toolbar Delete button, or Shift+Del, asks for a permanent delete with the same confirmation.

## Adding a method

1. Add a variant to `Method` and `MethodPref`; give it the next free `code`. Extend `label`, `program`, `from_setting` and `MethodPref::method`.
2. Detect it in `installed()`. If it only suits some routes, filter it in `candidates()`.
3. Build its command line in `command()` and its parser in `parse()`. If it reports on stderr, return true from `reports_on_stderr`. Define success in `succeeded()`.
4. Add the option to `transfer.method` in `src/settings/schema.ts`, the union types in `src/api.ts` and the label in `Transfers.tsx` (`METHOD_LABEL`).
5. Unit-test the command and the parser (see `method.rs` tests); if the tool exists on CI, add an end-to-end test like the rsync one in `engine.rs`.

## Benchmarks

`crates/fast-core/examples/transfer_bench.rs` copies three data sets with the
engine (auto, built-in, one stream), `cp -R` and `rsync -a`, and prints the
median of the runs. Point it at scratch folders on the drives you care about:

    cargo run --release -p fast-core --example transfer_bench -- SRC_DIR DEST_DIR [--runs 3] [--big-mb 512] [--small 20000]

It drops the page cache between runs when it can (Linux, root) and says so.

Result on one Linux cloud VM (2 vCPU, virtual disk, `--runs 3 --big-mb 256 --small 10000`).
**This is one machine, not an SSD, an HDD or a network share; do not read it as a general claim.**

| Data set | Engine (auto) | Engine (best variant) | `cp -R` | `rsync -a` |
|---|---|---|---|---|
| 10,000 small files (38 MB) | 0.52 s | 0.41 s (one stream) | 0.65 s | 0.65 s |
| 1,000 mixed files (1.2 GB) | 0.67 s | 0.59 s (built-in) | 0.76 s | 1.71 s |
| 4 large files (1 GB) | 0.71 s | 0.65 s (built-in) | **0.52 s** | 1.51 s |

On this machine the engine beats `cp` and `rsync` on small and mixed folders and
loses to `cp` by about 20% on a few large files. Run-to-run noise is large on a
shared VM; differences under about 10% mean nothing.

What the benchmark found and fixed:

* The progress ticker slept its whole interval (500 ms) after the workers had
  finished, adding up to half a second to every job. It now stops as soon as the
  workers are done.
* The speed probe ran even when the job would finish in well under the time the
  probe costs. It now runs only when the job is guessed to take over ten seconds.
* On Linux, large files are copied with `copy_file_range` in chunks (falling back
  to read/write when the filesystem refuses), and files are no longer preallocated
  with `set_len` on Unix, where it made copies slower.

Not yet measured: SSD against HDD, SMB and NFS shares, Windows, macOS, and
Explorer or Finder as the baseline.
