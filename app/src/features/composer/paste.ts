import { composerIpc, MAX_PASTE_BYTES, toCoreError } from "../../services/ipc";

/** What pasting HTML into the editor did (the core cleaned it, or it went in as plain text). */
export type PasteOutcome =
  | { kind: "cleaned"; html: string; imagesRemoved: number; formattingSimplified: boolean }
  /** Too large for the core's cleaning (over 2 MiB) or refused as such: the plain text went in. */
  | { kind: "plain"; text: string };

/**
 * Pasted HTML (Word, web pages) never enters the editor as it is: the core's allowlist removes
 * scripts, styles, images and external references first (`cleanPastedHtml`). Too large or
 * refused (`invalid_argument`) → the clipboard's plain text instead. Other failures reject.
 */
export async function cleanPaste(html: string, text: string): Promise<PasteOutcome> {
  if (new TextEncoder().encode(html).length > MAX_PASTE_BYTES) return { kind: "plain", text };
  try {
    const cleaned = await composerIpc.cleanPastedHtml(html);
    return { kind: "cleaned", ...cleaned };
  } catch (e) {
    const error = toCoreError(e);
    if (error.code === "invalid_argument") return { kind: "plain", text };
    throw error;
  }
}
