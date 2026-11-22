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
 * A size in bytes for people: "512 byte", "1.2 kB", "5 MB" in the UI language (Intl), on a
 * 1024 base like the size limit in the settings.
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
    unitDisplay: "short",
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
