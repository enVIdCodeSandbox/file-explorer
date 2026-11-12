# Theming

Open **Settings → Themes** and type CSS. It is applied as you type, on top of
the built-in theme, and saved with your settings.

Most themes only need to override variables. Everything in the app is drawn
from the variables below, which are defined in `src/styles/tokens.css`.

## Quick examples

```css
/* Warm accent, squarer corners */
:root {
  --accent: #ff8a3d;
  --radius: 4px;
  --radius-small: 2px;
}
```

```css
/* A full dark palette */
:root[data-theme="dark"] {
  --bg: #0f1117;
  --surface: #161a22;
  --surface-raised: #1d222c;
  --text: #e6e9ef;
  --text-muted: #8b93a3;
  --icon-folder: #7aa2f7;
}
```

```css
/* Bold folder names, taller rows, wider sidebar */
:root { --row-height: 30px; --sidebar-width: 260px; }
.file-row[data-kind="folder"] .file-name { font-weight: 600; }
```

Scope a rule to one colour mode with `:root[data-theme="dark"]` or
`:root[data-theme="light"]`, and to a density with
`:root[data-density="compact"]` or `"comfortable"`.

## Variables

| Variable | Used for |
|---|---|
| `--font`, `--font-mono` | Interface and monospace fonts |
| `--font-size`, `--font-size-small` | Base and secondary text size |
| `--radius`, `--radius-small` | Corner radius of panels and of rows, buttons |
| `--row-height` | Height of list rows |
| `--sidebar-width`, `--details-width` | Pane widths |
| `--accent` | Selection, focus, progress, switches. Also set by Settings → Appearance; CSS here wins |
| `--accent-text` | Text on accent-coloured buttons |
| `--danger`, `--warning` | Destructive actions and errors; warnings |
| `--bg` | Window background, tab strip, input fields |
| `--surface` | Toolbar, content area, active tab |
| `--surface-raised` | Menus, dialogs, cards |
| `--surface-hover`, `--surface-active` | Hover and pressed/selected backgrounds |
| `--border`, `--border-strong` | Dividers; outlines of menus and fields |
| `--text`, `--text-muted` | Primary and secondary text |
| `--selection`, `--selection-border` | Selected rows and tiles |
| `--shadow` | Shadow under menus and dialogs |
| `--icon-folder`, `--icon-file`, `--icon-page` | Folder colour, generic file outline, file page fill |
| `--treemap-text` | Labels on the disk usage map |

## Structure and class names

These names are the theming interface and will be kept stable.

```
.app[data-os="windows|macos|linux"]
├── .tab-bar
│   ├── .tab-group-chip
│   ├── .tab[data-active][data-grouped]      (--group-color is set on .tab-slot)
│   │   ├── .tab-title
│   │   └── .tab-close
│   └── .tab-new
├── .toolbar
│   ├── .nav-buttons
│   ├── .address-bar  → .breadcrumbs → .crumb
│   ├── .search-box
│   └── .tool-buttons → .settings-button
├── .workspace
│   ├── .sidebar → .side-heading, .side-item[data-active]
│   ├── .content
│   │   ├── .file-view[data-view="details|grid"]
│   │   │   ├── .file-header → .column-header[data-column][data-sorted]
│   │   │   ├── .file-row / .file-tile
│   │   │   │     [data-selected] [data-cut] [data-hidden]
│   │   │   │     [data-kind="folder|image|video|audio|archive|code|doc|sheet|slides|exe|file"]
│   │   │   │   ├── .cell.cell-name → .file-icon, .file-name
│   │   │   │   └── .cell.cell-date, .cell-type, .cell-size
│   │   │   └── .status-bar
│   │   ├── .home-view → .pin-card, .simple-row
│   │   ├── .drives-view → .drive-card[data-offline][data-nearly-full]
│   │   │                    → .meter → .meter-fill, .donut
│   │   ├── .search-view → .search-summary, .search-row
│   │   └── .analyzer-view → .treemap, .largest-row
│   └── .details-pane → .details-preview, .details-title, .details-list
├── .transfers → .transfer-card[data-phase] → .meter, .transfer-rate[data-display]
├── .context-menu → .menu-item[data-menu-id][data-danger], .menu-separator
├── .settings → .settings-nav, .settings-body → .setting[data-setting="…"]
├── .dialog
└── .toast
```

Useful hooks:

* `[data-kind]` on rows and tiles lets you style by file type.
* `[data-menu-id="delete"]` targets one right-click entry.
* `.transfer-rate[data-display="bytes|files|mixed"]` reflects which speed unit is showing.
* `.drive-card[data-nearly-full]` is set above 90% used.

## If a theme breaks the app

Custom CSS cannot run code or reach the network (the content security policy
blocks remote resources), but it can hide the interface. To recover, open
`settings.json` in the app's config folder and clear `appearance.customCss`:

| OS | Location |
|---|---|
| Windows | `%APPDATA%\dev.envid.fastfileexplorer\settings.json` |
| macOS | `~/Library/Application Support/dev.envid.fastfileexplorer/settings.json` |
| Linux | `~/.config/dev.envid.fastfileexplorer/settings.json` |

## Themes, fonts, imports and layout (Settings > Themes / Layout)

- **Built-in themes**: 13, chosen with `data-theme`; each is a block in `src/styles/tokens.css`. Look options (`data-menu`, `data-icons`, `data-folder`, `data-bg`) are independent of the theme.
- **Imports**: fonts (`ttf/otf/woff/woff2`), CSS files and background pictures are copied into the settings folder (`fonts/`, `css/`, `images/`). Imported CSS is applied in the order listed and can be switched on or off per file. Remove a file and it stops being used.
- **Layout**: tabs, address bar, command bar, navigation pane and details pane are panels. Each can be docked top/bottom/left/right, floated, popped out behind a button, collapsed or hidden. Use presets, the table in Settings > Layout, or "Arrange on screen" and drag panel handles. Stored in `settings.appearance.layout`.
- **Advanced** (Settings > Advanced): dozens of tuning options (`settings.advanced`), remappable shortcuts (`settings.keys`), your own right-click programs (`settings.customActions`, started directly, never through a shell), and a raw JSON settings editor.
