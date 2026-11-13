// Inline SVG icons: no icon font or image requests, and they inherit
// `currentColor`, so themes can recolour them with CSS alone.

import type { CSSProperties, ReactNode } from "react";
import { useStore } from "../store";
import { fileTypeOf } from "../util/fileTypes";
import { FLUENT } from "./fluentIcons";

const PATHS: Record<string, string> = {
  back: "M10 3 5 8l5 5M5 8h9",
  forward: "M6 3l5 5-5 5M11 8H2",
  up: "M3 8l5-5 5 5M8 3v11",
  refresh: "M13 8a5 5 0 1 1-1.5-3.5M13 2v3h-3",
  clock: "M8 13A5 5 0 1 0 8 3a5 5 0 0 0 0 10zM8 5.5V8l1.8 1.2",
  search: "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zm3.5-1.5L14 14",
  settings:
    "M8 10.2a2.2 2.2 0 1 0 0-4.4 2.2 2.2 0 0 0 0 4.4zM13 8l1.4-1-1.2-2.2-1.7.5-1-.8L10.2 3H7.8l-.3 1.5-1 .8-1.7-.5L3.6 7 5 8l-1.4 1 1.2 2.2 1.7-.5 1 .8.3 1.5h2.4l.3-1.5 1-.8 1.7.5L14.4 9z",
  close: "M4 4l8 8M12 4l-8 8",
  plus: "M8 3v10M3 8h10",
  home: "M2 8l6-5.5L14 8M4 7v6.5h3V10h2v3.5h3V7",
  pc: "M2 3h12v8H2zM6 14h4M8 11v3",
  pin: "M9.5 2l4.5 4.5-2 .5-2.5 2.5.5 3L8 10.5 3.5 15M8 10.5 5.5 8l3-.5L11 5",
  terminal: "M2 3h12v10H2zM4.5 6l2.5 2-2.5 2M8.5 10.5h3",
  details: "M2 3h12v10H2zM10 3v10",
  grid: "M2 2h5v5H2zM9 2h5v5H9zM2 9h5v5H2zM9 9h5v5H9z",
  list: "M2 4h12M2 8h12M2 12h12",
  chart: "M2 2h12v12H2zM2 9h6V2M8 9v5M8 6h6",
  pause: "M5 3v10M11 3v10",
  play: "M4 3l9 5-9 5z",
  trash: "M3 4h10M6 4V2.5h4V4M4.5 4l.5 9.5h6l.5-9.5",
  chevron: "M6 4l4 4-4 4",
  drive: "M2 5h12v6H2zM4.5 8h.01M2 5l2-2.5h8L14 5",
  network: "M8 2v4M3 10h10M3 10v2M13 10v2M8 6v4M2 12h2v2H2zM12 12h2v2h-2zM7 12h2v2H7z",
  copy: "M5 5h8v9H5zM3 11V2h8",
  cut: "M4 3l8 10M12 3L4 13M3 12.5a1.5 1.5 0 1 0 0 .01M13 12.5a1.5 1.5 0 1 0 0 .01",
  paste: "M5.5 3H4v11h8V3h-1.5M6 2h4v2H6zM6.5 7.5h3M6.5 10h3",
  rename: "M2 13h12M3 10l7-7 2 2-7 7H3zM9 4l2 2",
  share: "M11 5.5a1.7 1.7 0 1 0 0-.01M5 9.7a1.7 1.7 0 1 0 0-.01M11 13.5a1.7 1.7 0 1 0 0-.01M6.5 8.8l3-2.6M6.5 10.6l3 2",
  newItem: "M8 3v10M3 8h10",
  sort: "M4 3v10M2 11l2 2 2-2M10 4h4M10 8h3M10 12h2",
  view: "M2 3h5v4H2zM9 3h5v4H9zM2 9h5v4H2zM9 9h5v4H9z",
  filter: "M2 3h12l-4.5 5.5V13l-3 1V8.5z",
  more: "M3.5 8h.01M8 8h.01M12.5 8h.01",
  undo: "M4 6h6a3 3 0 0 1 0 6H6M4 6l2.5-2.5M4 6l2.5 2.5",
  archive: "M2.5 3h11v3h-11zM3.5 6v7h9V6M6.5 9h3",
  redo: "M12 6H6a3 3 0 0 0 0 6h4M12 6L9.5 3.5M12 6L9.5 8.5",
  check: "M3 8.5l3.2 3L13 4.5",
  sidebar: "M2 3h12v10H2zM6 3v10",
  star: "M8 2l1.8 3.9 4.2.5-3.1 2.9.8 4.2L8 11.3 4.3 13.5l.8-4.2L2 6.4l4.2-.5z",
  minimize: "M3 8h10",
  maximize: "M3.5 3.5h9v9h-9z",
  restore: "M5.5 5.5h7v7h-7zM3.5 10.5v-7h7",
  popin: "M7 9H3v4M3 13l6-6M5 3h8v8",
  popout: "M9 3h4v4M13 3L7 9M11 9v4H3V5h4",
  expand: "M3 9l5 4 5-4M3 4l5 4 5-4",
  collapse: "M3 7l5-4 5 4M3 12l5-4 5 4",
  info: "M8 13A5 5 0 1 0 8 3a5 5 0 0 0 0 10zM8 7.5v3M8 5.6v.01",
  folderOpen: "M2 12V3h4l1.5 1.5H14V6M2 12l2-5h11l-2 5z",
  shield: "M8 2l5 1.8v4c0 3-2.2 5-5 6.2C5.2 12.8 3 10.8 3 7.8v-4z",
  lock: "M4 7h8v6.5H4zM5.5 7V5a2.5 2.5 0 0 1 5 0v2",
  cpu: "M4.5 4.5h7v7h-7zM6.5 6.5h3v3h-3zM6 2v2.5M10 2v2.5M6 11.5V14M10 11.5V14M2 6h2.5M2 10h2.5M11.5 6H14M11.5 10H14",
  profile: "M8 8a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zM3 14c0-2.5 2.2-4 5-4s5 1.5 5 4",
  preset: "M2 4.5h12M2 8h12M2 11.5h12M10 3v3M5 6.5v3M11 10v3",
  memory: "M2 5h12v6H2zM4.5 11v2M8 11v2M11.5 11v2M5 7.5h.01M8 7.5h.01M11 7.5h.01",
  eject: "M8 3l5 6H3zM3 12h10",
};

export function Icon({ name, size = 16, style }: { name: string; size?: number; style?: CSSProperties }) {
  const fluent = useStore((s) => s.settings?.appearance.style.icons === "fluent");
  const f = fluent ? FLUENT[name] : undefined;
  if (f) {
    return (
      <svg className="icon" data-set="fluent" width={size} height={size} viewBox={`0 0 ${f.s} ${f.s}`} fill="currentColor" style={style} aria-hidden="true">
        {f.d.map((d, i) => (
          <path key={i} d={d} />
        ))}
      </svg>
    );
  }
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={style}
      aria-hidden="true"
    >
      <path d={PATHS[name] ?? ""} />
    </svg>
  );
}

const KIND_COLORS: Record<string, string> = {
  folder: "var(--icon-folder)",
  image: "#56b6c2",
  video: "#c678dd",
  audio: "#e5c07b",
  archive: "#d19a66",
  code: "#61afef",
  doc: "#e06c75",
  sheet: "#98c379",
  slides: "#e58e5a",
  exe: "#abb2bf",
  file: "var(--icon-file)",
};

const GLYPH_PATHS: Record<string, React.ReactNode> = {
  image: <path d="M5 11.5l2-2.6 1.6 1.7 1.1-1.2 1.8 2.1z" />,
  video: <path d="M6.5 7.2v4.6l4-2.3z" />,
  audio: <path d="M9.6 6.5v4.3a1.3 1.3 0 1 1-.9-1.2V7.6L7 8v3.1" />,
  archive: <path d="M8 3v1.2M8 5.4v1.2M8 7.8v1.2M6.6 10.2h2.8v1.8H6.6z" />,
  code: <path d="M6.4 7.3L4.8 9l1.6 1.7M9.6 7.3L11.2 9 9.6 10.7" />,
  data: <path d="M6.6 6.5c-.9 0-.9.8-.9 1.3s0 1-.7 1.2c.7.2.7.7.7 1.2s0 1.3.9 1.3M9.4 6.5c.9 0 .9.8.9 1.3s0 1 .7 1.2c-.7.2-.7.7-.7 1.2s0 1.3-.9 1.3" />,
  text: <path d="M5 7h6M5 9h6M5 11h4" />,
  doc: <path d="M5 7h6M5 9h6M5 11h3.5" />,
  sheet: <path d="M5 7h6v5H5zM5 9.5h6M8 7v5" />,
  slides: <path d="M5 11.5V9M8 11.5V7M11 11.5V8.4" />,
  exe: <path d="M5 7h6v5H5zM5 8.6h6" />,
  disk: <path d="M8 7a2.6 2.6 0 1 0 0 5.2A2.6 2.6 0 0 0 8 7zm0 1.9v.4" />,
  font: <path d="M5.2 12l2-5.2L9.2 12M5.9 10.3h2.6M10.4 9.5h1.4" />,
  model: <path d="M8 6.6l3 1.6v3.2L8 13l-3-1.6V8.2zM8 9.8V13M5 8.2l3 1.6 3-1.6" />,
  key: <path d="M7 8.2a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zm1.2.7L11.5 6.7M10.3 7.5l.9.9" />,
  web: <path d="M8 6.8a2.7 2.7 0 1 0 0 5.4 2.7 2.7 0 0 0 0-5.4zM5.3 9.5h5.4M8 6.8c-1 1.6-1 3.8 0 5.4M8 6.8c1 1.6 1 3.8 0 5.4" />,
  db: <path d="M5.2 7.4c0-.8 5.6-.8 5.6 0v4c0 .8-5.6.8-5.6 0zM5.2 9.4c0 .8 5.6.8 5.6 0" />,
  pdf: <path d="M5 11.8c1.6-1 2.4-3 2.4-4.6 0-.6-.7-.6-.7.1 0 1.8 2.6 3.8 4.2 3.7.6 0 .5-.8-.1-.8-1.5 0-4 .3-5.8 1.6z" />,
  book: <path d="M5 6.8h2.6c.3 0 .4.1.4.4v5c0-.3-.1-.4-.4-.4H5zM11 6.8H8.4c-.3 0-.4.1-.4.4v5c0-.3.1-.4.4-.4H11z" />,
  link: <path d="M7 9.8a2 2 0 0 1 0-2.8l.8-.8a2 2 0 0 1 2.8 2.8M9 8.2a2 2 0 0 1 0 2.8l-.8.8A2 2 0 0 1 5.4 9" />,
  config: <path d="M8 8.2a1.4 1.4 0 1 0 0 2.8 1.4 1.4 0 0 0 0-2.8zM8 6.6v1M8 11v1M5.4 9.6h1M9.6 9.6h1" />,
  log: <path d="M5 7h1M7 7h4M5 9h1M7 9h4M5 11h1M7 11h3" />,
  shell: <path d="M5 7.4l2 1.6-2 1.6M8.4 11h2.6" />,
  package: <path d="M8 6.4l3 1.4v3.6L8 12.8l-3-1.4V7.8zM8 9.2v3.6M5 7.8l3 1.4 3-1.4" />,
};

/** File-type glyph: a folder shape, or a page drawn per extension/category. */
/** Well-known folders get their own mark on the folder shape. Keys are the English place names. */
const PLACE_MARKS: Record<string, { color: string; mark: ReactNode }> = {
  downloads: { color: "#2fa56b", mark: <path d="M8 6.4v4.2M6.2 8.9 8 10.7l1.8-1.8" /> },
  documents: { color: "#4f8fe0", mark: <path d="M6.2 6.3h3.6M6.2 8.1h3.6M6.2 9.9h2.2" /> },
  pictures: { color: "#2aa3b8", mark: <path d="m5.6 10.7 1.8-2.2 1.2 1.3.9-1 1.1 1.9zM6.4 6.9h.01" /> },
  music: { color: "#d4609a", mark: <path d="M9.6 6.1v4M9.6 6.1 7 6.7v3.7M7 10.4a.9.9 0 1 1-.01 0M9.6 10.1a.9.9 0 1 1-.01 0" /> },
  videos: { color: "#8b6bd6", mark: <path d="M6.9 6.4v4.2l3.3-2.1z" /> },
  desktop: { color: "#3b82d6", mark: <path d="M5.4 6.2h5.2v3.4H5.4zM7 11h2" /> },
};

export function FileIcon({ kind, size = 16, name }: { kind: string; size?: number; name?: string }) {
  if (kind === "folder" && name && PLACE_MARKS[name.toLowerCase()]) {
    const { color, mark } = PLACE_MARKS[name.toLowerCase()];
    return (
      <svg className="file-icon" data-kind="folder" data-place={name.toLowerCase()} width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
        <path d="M1.5 4.2c0-.7.5-1.2 1.2-1.2h3.1l1.4 1.4h6.1c.7 0 1.2.5 1.2 1.2v6.2c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2z" fill={color} />
        <path d="M1.5 5.6h13v6.2c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2z" fill="rgba(255,255,255,.18)" />
        <g fill="none" stroke="#fff" strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round">{mark}</g>
      </svg>
    );
  }
  if (kind === "folder") {
    const color = KIND_COLORS.folder;
    return (
      <svg className="file-icon" data-kind="folder" width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
        <path d="M1.5 4.2c0-.7.5-1.2 1.2-1.2h3.1l1.4 1.4h6.1c.7 0 1.2.5 1.2 1.2v6.2c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2z" fill={color} />
        <path d="M1.5 6.2h13v5.6c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2z" fill="rgba(255,255,255,.22)" />
      </svg>
    );
  }
  const type = name ? fileTypeOf(name, kind) : undefined;
  const color = type?.color ?? KIND_COLORS[kind] ?? KIND_COLORS.file;
  const glyph = type ? GLYPH_PATHS[type.glyph] : undefined;
  const showLabel = !!type && size >= 28;
  return (
    <svg className="file-icon" data-kind={kind} data-ext={type?.label} width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
      <path d="M3.5 1.5h6l3 3v10h-9z" fill="var(--icon-page)" stroke={color} strokeWidth="1" strokeLinejoin="round" />
      <path d="M9.5 1.5v3h3" fill="none" stroke={color} strokeWidth="1" strokeLinejoin="round" />
      {glyph && !showLabel ? (
        <g fill="none" stroke={color} strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round">
          {glyph}
        </g>
      ) : showLabel ? (
        <>
          <rect x="2" y="9.2" width={12} height="4" rx="1" fill={color} />
          <text x="8" y="12.2" textAnchor="middle" fontSize={type.label.length > 3 ? 2.9 : 3.6} fontWeight="700" fill="#fff" fontFamily="system-ui, sans-serif">
            {type.label}
          </text>
          <g fill="none" stroke={color} strokeWidth="1" strokeLinecap="round" strokeLinejoin="round" transform="translate(0 -3.2) scale(1)">
            {glyph}
          </g>
        </>
      ) : (
        <>
          <rect x="5" y="8" width="6" height="1.2" rx=".6" fill={color} />
          <rect x="5" y="10.5" width="4" height="1.2" rx=".6" fill={color} />
        </>
      )}
    </svg>
  );
}
