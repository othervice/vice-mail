import {
  File,
  FileArchive,
  FileAudio,
  FileExclamationPoint,
  FileImage,
  FileText,
  FileVideo,
  type LucideIcon,
} from "lucide-react";
import type { Address } from "../../services/ipc";

/** What the UI calls an address: its display name, else the address itself; null if both are empty. */
export function addressName(address: Address): string | null {
  return address.name?.trim() || address.email.trim() || null;
}

const UNITS = ["byte", "kilobyte", "megabyte", "gigabyte"] as const;

/**
 * A size in bytes for people: "512 bytes", "1.2 kB", "5 MB" in the UI language (Intl), on a
 * 1024 base like the size limit in the settings. Plain bytes are written out: the short English
 * form is "512 byte" without a plural (German "512 Byte" is the same either way).
 */
export function formatSize(bytes: number, language: string): string {
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: UNITS[unit] ?? "byte",
    unitDisplay: unit === 0 ? "long" : "short",
    maximumFractionDigits: unit === 0 || value >= 10 ? 0 : 1,
  }).format(value);
}

/** The short type of an attachment: its extension in capitals ("PDF"), else the MIME type. */
export function fileType(name: string, mimeType: string): string {
  const match = /\.([a-z0-9]{1,8})$/i.exec(name);
  return match?.[1] ? match[1].toUpperCase() : mimeType || "application/octet-stream";
}

const ARCHIVES = /(zip|x-7z|x-rar|x-tar|gzip|x-bzip|x-xz|compressed)/;

export type AttachmentKind = "executable" | "image" | "audio" | "video" | "document" | "archive" | "other";

/** Icons per kind of file. Decorative: the type is always written next to them. */
export const ATTACHMENT_ICONS: Record<AttachmentKind, LucideIcon> = {
  executable: FileExclamationPoint,
  image: FileImage,
  audio: FileAudio,
  video: FileVideo,
  document: FileText,
  archive: FileArchive,
  other: File,
};

export function attachmentKind(mimeType: string, executable: boolean): AttachmentKind {
  if (executable) return "executable";
  const [major = ""] = mimeType.split("/");
  if (major === "image") return "image";
  if (major === "audio") return "audio";
  if (major === "video") return "video";
  if (major === "text" || mimeType === "application/pdf") return "document";
  if (ARCHIVES.test(mimeType)) return "archive";
  return "other";
}

/**
 * Stage 23, step 3 (decision 0084): the colour family of an attachment card (tokens `file-*`):
 * PDF, documents, spreadsheets, images, archives; anything else (and programs) is plain.
 */
export type FileTint = "pdf" | "doc" | "sheet" | "image" | "archive" | null;

const SHEETS = /^(xls|xlsx|xlsm|ods|csv|tsv|numbers)$/;
const DOCUMENTS = /^(doc|docx|odt|rtf|txt|md|pages|ppt|pptx|odp|key)$/;
const ARCHIVE_NAMES = /^(zip|7z|rar|tar|gz|tgz|bz2|xz)$/;

export function fileTint(name: string, mimeType: string, executable: boolean): FileTint {
  if (executable) return null;
  const extension = /\.([a-z0-9]{1,8})$/i.exec(name)?.[1]?.toLowerCase() ?? "";
  if (extension === "pdf" || mimeType === "application/pdf") return "pdf";
  if (SHEETS.test(extension) || /spreadsheet|ms-excel|text\/csv/.test(mimeType)) return "sheet";
  if (mimeType.startsWith("image/")) return "image";
  if (ARCHIVE_NAMES.test(extension) || ARCHIVES.test(mimeType)) return "archive";
  if (DOCUMENTS.test(extension) || /wordprocessing|msword|presentation|opendocument\.text|^text\//.test(mimeType)) return "doc";
  return null;
}

/** The badge's letters: the first three of the extension in capitals ("XLS" for `.xlsx`), if any. */
export function fileBadge(name: string): string | null {
  const extension = /\.([a-z0-9]{1,8})$/i.exec(name)?.[1];
  return extension ? extension.slice(0, 3).toUpperCase() : null;
}
