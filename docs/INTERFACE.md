# Interface internals

How the parts of `src/` that are not obvious work. For the layers and the file map see ARCHITECTURE.md.

## Settings screen

The screen is generated from `src/settings/schema.ts`. `SECTIONS` lists the pages in order; each `SettingDef` has `key` (a dotted path into the `Settings` object, e.g. `transfer.method`), `section`, `label`, optional `description` and `keywords` (searched), and a `control`: `toggle`, `select`, `number`, `text` or a custom one. `advanced.ts` holds entries shown only under Advanced. Values are read with `getSetting` and written with `setSetting` through `store.updateSettings`, which saves immediately and broadcasts `settings-changed` to all windows.

Pages with more than plain controls (Advanced: shortcuts, custom actions, maintenance; About; Documentation) are rendered in `SettingsView.tsx` after the generated entries.

## Locations

A location is a string: a filesystem path, or `fast://home`, `fast://thispc`, `fast://search?q=…&scope=…`, `fast://analyze?path=…`. Helpers are in `src/util/path.ts`. Windows paths keep their backslashes; never assume `/`.

## Selection model (`FileView.tsx`)

Selection follows the spreadsheet convention:

* `anchor`: the last row clicked without Shift.
* `base`: the selection committed before the current range.
* Click: base = {}, selection = {row}, anchor = row.
* Ctrl+click: toggle the row in the selection; base = that selection; anchor = row.
* Shift+click: selection = base ∪ range(anchor, row). Click Shift again elsewhere and only the range changes, base stays.
* Ctrl+Shift+click: add range(anchor, row) to the selection and commit it as the new base.
* Anything that changes the selection elsewhere (select all, keyboard, rubber band) resets `base` to the current selection.

This is why "select a block, Ctrl+click another, Shift+click a third" keeps the first block.

## Drag and drop (`src/dnd.ts`)

All drags inside the app use pointer events, not HTML5 drag and drop. That leaves the window's native drop handler (`dragDropEnabled: true`) free for files dragged in from other programs; see "Files dragged in" below.

* `beginDrag(e, payload)` is called from `onPointerDown` on a draggable (file rows, pinned items, section headings, tabs). Nothing starts until the pointer has moved a few pixels; a plain click is unaffected, and a click right after a drag is swallowed.
* While dragging, the element under the pointer is looked up with `elementsFromPoint` and the nearest ancestor with `data-drop` decides what happens:

| `data-drop` | extra attribute | Meaning |
|---|---|---|
| `folder` | `data-drop-path` | drop into this folder (rows, breadcrumbs, sidebar entries, empty view area) |
| `pinitem` | `data-pin-index` | a pinned folder: edges (25%/75%) insert before/after, the middle drops into the folder |
| `pins` | `data-pin-count` | the Pinned heading or its empty area: pin the folder |
| `section` | `data-section` | a sidebar section: reorder sections |
| `tab` | `data-tab-id`, `data-drop-path` | a tab: hovering 700 ms opens it; dropping files moves/copies into its folder |

* The target gets `data-drop-over`; insert positions get `data-insert="before|after"`. Both are set on the DOM directly (no React render per mouse move).
* The view autoscrolls when the pointer nears the top or bottom edge of a scroller.
* `resolveFolderDrop(payload, targetPath, mods, volumeOf)` is a pure function that decides what a drop to a folder does; it is unit-tested in `util.test.ts`:
  * same volume → **move**; different volume → **copy**
  * Ctrl → copy, Shift → move
  * dropping into the folder the items are already in → nothing (with Ctrl, a duplicate)
  * a folder onto itself or one of its own descendants → refused
* Pointer leaving the window with files in hand starts a native drag-out (below).

## Files dragged in (`src/externalDrop.ts`)

`getCurrentWebview().onDragDropEvent` reports enter, over, drop and leave with absolute file paths and a physical-pixel position (divided by `devicePixelRatio` to get CSS pixels). On over, `elementsFromPoint` finds the nearest `data-drop="folder"` or `data-drop="tab"` element with a `data-drop-path` (the same targets as in-app drags), marks it, and shows the floating label "Copy to …". On drop the files go to `startTransfer(paths, dest, "copy")`. With no such element under the pointer the destination is the active tab's folder; for `fast://` locations there is none and a toast says so. Files this app itself started dragging out (`noteOwnDrag`) are ignored if they come back to its own window. External files are always copied, as other programs' drops cannot be told apart from a move.

## Pins and sidebar order

Pins are a list in the session (`togglePin`, `pinAt`, `movePin` in `store.ts`). Section order is `advanced["sidebar.order"]`, a permutation of the six sections, read with `sidebarOrder(settings)`. Dragging a heading rewrites it.

## System clipboard and drag-out

* **Copy / Cut** keeps the internal clipboard (so Cut can show items dimmed) and also writes the paths to the system clipboard as files (`clipboard_set_files`, crate `clipboard-rs`). Other apps can paste them.
* **Paste** (`store.pasteInto(dest)`) reads the system clipboard. If it holds exactly our own paths, the internal operation (copy or move) is used; if it holds files from another app, they are copied. Operating systems do not let a clipboard writer mark "cut" for other applications, so cut across apps pastes as a copy.
* **Drag-out** (`drag_out`, crate `drag`): when a drag with files leaves the window, the native drag session starts with the file list in copy mode. Desktop only; mobile builds have stubs.

## Tabs and windows

* A tab can be dragged out of the strip to become its own window (`openWindow(location, x, y)` places it under the pointer).
* Dropping a tab on another window's tab bar calls `drop_tab`: the shell hit-tests all windows by screen position and emits `tab-drop` to the window under the pointer, which inserts the tab at the slot under the pointer.
* Tabs are `tabs.width` wide and shrink together to `tabs.minWidth`; below that the strip scrolls.
* A popped-out window has no Back button: it was created from a tab, there is nothing to go back to.

## Details pane

One item: `get_details` (kind, size, dates, type, folder item count). Several items: `MultiSummary` totals the first 500 with six concurrent `get_details` calls: file and folder counts, size, size on disk, largest file, location, oldest and newest change, read-only/hidden counts and a type breakdown. "Measure folders" runs `folder_stats` on each selected folder and sums the result.

## Theming and sizes

See THEMING.md. All sizes pass through `formatSize` (`src/util/format.ts`) so the unit setting applies everywhere.
