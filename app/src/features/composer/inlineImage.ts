import { mergeAttributes, Node } from "@tiptap/react";

/** `cid:<contentId>` → `data:` URL, only for showing an image in the editor. */
export type ImageSources = Readonly<Record<string, string>>;

/** Widths the size menu offers, in px (`null` = the image's own width, capped by the editor). */
export const IMAGE_SIZES = { small: 160, medium: 320, large: 640, original: null } as const;
export type ImageSize = keyof typeof IMAGE_SIZES;

/** The menu entry of a width (a width set elsewhere shows as the nearest one). */
export function imageSizeOf(width: unknown): ImageSize {
  if (typeof width !== "number" || !Number.isFinite(width)) return "original";
  if (width <= 240) return "small";
  if (width <= 480) return "medium";
  return "large";
}

/** The core accepts 1–4096 px (decision 0023). */
function parseWidth(element: HTMLElement): number | null {
  const raw = element.getAttribute("width") ?? /(\d+)px/.exec(element.style.width)?.[1] ?? null;
  if (raw === null) return null;
  const width = Number.parseInt(raw, 10);
  return Number.isFinite(width) && width >= 1 && width <= 4096 ? width : null;
}

const CID = /^cid:(.+)$/;

/**
 * Inline images of a draft or signature: `<img src="cid:…" alt="" width="…">` in the editor
 * model and in the saved HTML. A node view shows the image from its `data:` URL (from
 * `inlineImageData` or `signatureImageData`); that URL never reaches the saved HTML. Other
 * images (web addresses, `data:`) are not part of the schema, so they never get in.
 */
export function createInlineImage() {
  let sources: ImageSources = {};
  const views = new Set<{ img: HTMLImageElement; contentId: string }>();
  const show = (view: { img: HTMLImageElement; contentId: string }) => {
    const url = sources[view.contentId];
    if (url) view.img.src = url;
    else view.img.removeAttribute("src");
  };

  const extension = Node.create({
    name: "inlineImage",
    group: "inline",
    inline: true,
    atom: true,
    draggable: true,
    selectable: true,
    addAttributes: () => ({
      src: { default: null },
      alt: { default: "" },
      width: {
        default: null,
        parseHTML: parseWidth,
        renderHTML: (attributes: Record<string, unknown>) =>
          typeof attributes["width"] === "number" ? { width: String(attributes["width"]) } : {},
      },
    }),
    parseHTML: () => [{ tag: 'img[src^="cid:"]' }],
    renderHTML: ({ HTMLAttributes }) => ["img", mergeAttributes(HTMLAttributes)],
    addNodeView:
      () =>
      ({ node, editor, getPos }) => {
        const img = document.createElement("img");
        img.className = "composer-inline-image";
        img.draggable = true;
        // A click selects the image (for the size menu), also where the engine would not.
        img.addEventListener("click", () => {
          const pos = getPos();
          if (typeof pos === "number" && editor.isEditable) editor.commands.setNodeSelection(pos);
        });
        let view = { img, contentId: CID.exec(String(node.attrs["src"] ?? ""))?.[1] ?? "" };
        const apply = (attrs: Record<string, unknown>) => {
          img.alt = typeof attrs["alt"] === "string" ? attrs["alt"] : "";
          if (typeof attrs["width"] === "number") img.width = attrs["width"];
          else img.removeAttribute("width");
          img.dataset["contentId"] = view.contentId;
        };
        apply(node.attrs);
        views.add(view);
        show(view);
        return {
          dom: img,
          update: (next) => {
            if (next.type.name !== "inlineImage") return false;
            const contentId = CID.exec(String(next.attrs["src"] ?? ""))?.[1] ?? "";
            if (contentId !== view.contentId) {
              views.delete(view);
              view = { img, contentId };
              views.add(view);
            }
            apply(next.attrs);
            show(view);
            return true;
          },
          destroy: () => {
            views.delete(view);
          },
        };
      },
  });

  return {
    extension,
    setSources: (next: ImageSources) => {
      sources = next;
      for (const view of views) show(view);
    },
  };
}
