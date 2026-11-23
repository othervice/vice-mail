import { Extension } from "@tiptap/react";

/** Alignment the core keeps on `<p>` and `<h1>`–`<h3>` (`style="text-align: …"`); left is no style. */
export type Alignment = "left" | "center" | "right" | "justify";

const KEPT: readonly string[] = ["center", "right", "justify"];

/**
 * Text alignment without another package: an attribute on paragraphs and headings that reads
 * and writes exactly `style="text-align: center|right|justify"` (what the allowlist keeps).
 */
export const TextAlign = Extension.create({
  name: "textAlign",
  addGlobalAttributes: () => [
    {
      types: ["paragraph", "heading"],
      attributes: {
        textAlign: {
          default: null,
          parseHTML: (element: HTMLElement) => {
            const value = element.style.textAlign;
            return KEPT.includes(value) ? value : null;
          },
          renderHTML: (attributes: Record<string, unknown>) => {
            const value = attributes["textAlign"];
            return typeof value === "string" && KEPT.includes(value) ? { style: `text-align: ${value}` } : {};
          },
        },
      },
    },
  ],
});
