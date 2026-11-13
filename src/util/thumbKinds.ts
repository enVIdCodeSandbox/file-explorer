// Which kind of preview a file gets. Mirrors the lists in crates/fast-core/src/thumbs.rs
// (a test keeps the two in step).

export const IMAGE_EXTS = ["jpg", "jpeg", "png", "gif", "webp", "bmp"];
export const DOCUMENT_EXTS = ["pdf", "docx", "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "epub", "svg"];
export const VIDEO_EXTS = ["mp4", "m4v", "mov", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg", "3gp", "mts", "m2ts"];

export type ThumbKind = "images" | "documents" | "videos";

export function thumbKind(ext: string): ThumbKind | null {
  const e = ext.toLowerCase();
  if (IMAGE_EXTS.includes(e)) return "images";
  if (DOCUMENT_EXTS.includes(e)) return "documents";
  if (VIDEO_EXTS.includes(e)) return "videos";
  return null;
}

/** The view a thumbnail is for: one of the folder views, or the details pane. */
export type ThumbView = "details" | "list" | "small" | "medium" | "large" | "xl" | "pane";

export const VIEW_SETTING: Record<ThumbView, string> = {
  details: "thumbs.inDetails",
  list: "thumbs.inList",
  small: "thumbs.inSmall",
  medium: "thumbs.inMedium",
  large: "thumbs.inLarge",
  xl: "thumbs.inXl",
  pane: "thumbs.inPane",
};

/** How many pixels to ask for, as a multiple of the size shown. */
export const QUALITY: Record<string, number> = { draft: 0.5, normal: 1, sharp: 2 };
