import type { Editor } from "@tiptap/react";
import type { Alignment } from "./textAlign";

/**
 * What the editor's toolbars do with the editor (the full bar of the signature and template
 * editors, the composer's compact bar of stage 23): block styles, alignment, links, clearing.
 */

/** The block styles the core keeps: paragraphs, headings 1 to 3 and code blocks. */
export const BLOCK_STYLES = ["paragraph", "heading1", "heading2", "heading3", "codeBlock"] as const;
export type BlockStyle = (typeof BLOCK_STYLES)[number];

export function blockStyleOf(editor: Editor): BlockStyle {
  for (const level of [1, 2, 3] as const) if (editor.isActive("heading", { level })) return `heading${String(level)}` as BlockStyle;
  if (editor.isActive("codeBlock")) return "codeBlock";
  return "paragraph";
}

export function setBlockStyle(editor: Editor, style: BlockStyle): void {
  const chain = editor.chain().focus();
  switch (style) {
    case "paragraph":
      chain.setParagraph().run();
      break;
    case "codeBlock":
      chain.setCodeBlock().run();
      break;
    default:
      chain.setHeading({ level: Number(style.slice(-1)) as 1 | 2 | 3 }).run();
  }
}

export const ALIGNMENT_IDS: readonly Alignment[] = ["left", "center", "right", "justify"];

export function alignmentOf(editor: Editor): Alignment {
  for (const align of ["center", "right", "justify"] as const) {
    if (editor.isActive({ textAlign: align })) return align;
  }
  return "left";
}

export function setAlignment(editor: Editor, align: Alignment): void {
  const value = align === "left" ? null : align;
  editor.chain().focus().updateAttributes("paragraph", { textAlign: value }).updateAttributes("heading", { textAlign: value }).run();
}

/**
 * Stage 23 (decision 0086): "Clear formatting": marks off, headings, lists, quotes and code
 * blocks back to paragraphs, alignment back to the start. Inline images stay.
 */
export function clearFormatting(editor: Editor): void {
  editor.chain().focus().unsetAllMarks().clearNodes().updateAttributes("paragraph", { textAlign: null }).run();
}

/** Only web and mail links; anything else (javascript:, file:) is refused. */
export function normaliseLink(input: string): string | null {
  const text = input.trim();
  if (text === "") return null;
  if (/^(https?:\/\/|mailto:)/i.test(text)) return text;
  if (/^[^\s:/]+@[^\s:/]+$/.test(text)) return `mailto:${text}`;
  if (/^[a-z][a-z0-9+.-]*:/i.test(text)) return null;
  return `https://${text}`;
}
