import { EditorContent, Extension, useEditor, useEditorState, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import {
  AlignCenter,
  AlignJustify,
  AlignLeft,
  AlignRight,
  Bold,
  Code,
  ImagePlus,
  Italic,
  Link2,
  List,
  ListOrdered,
  Redo2,
  Strikethrough,
  TextQuote,
  Underline,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useId, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import type { CoreError } from "../../services/ipc";
import { cleanPaste, type PasteOutcome } from "./paste";
import { createInlineImage, IMAGE_SIZES, imageSizeOf, type ImageSize, type ImageSources } from "./inlineImage";
import { TextAlign, type Alignment } from "./textAlign";
import "./composer.css";

/** What the composer (and the signature and template editors) can do with the editor. */
export interface RichEditorHandle {
  /** Puts editor HTML at the cursor (a filled template). */
  insertHtml: (html: string) => void;
  /** Puts `<img src="cid:…">` for each image at the cursor. */
  insertImages: (images: readonly { contentId: string; alt?: string }[]) => void;
  focus: () => void;
}

/** What a paste did: the core cleaned it (with notes), it went in as plain text, or it failed. */
export type PasteResult = PasteOutcome | { kind: "failed"; error: CoreError };

interface RichEditorProps {
  /** Editor HTML at the start; later changes come from the user only. */
  initialHtml: string;
  onChange: (html: string) => void;
  onBlur: () => void;
  label: string;
  disabled?: boolean;
  /** Replies start in the body, above the quote. */
  autoFocus?: boolean;
  /** `cid:` → `data:` URL, only to show inline images; the saved HTML keeps `cid:`. */
  imageSources?: ImageSources;
  /** Shows "Insert image" in the toolbar; the caller opens the core's file dialog. */
  onInsertImage?: () => void;
  handleRef?: Ref<RichEditorHandle | null>;
  /** What happened to pasted HTML (images removed, formatting simplified, failure). */
  onPasteResult?: (result: PasteResult) => void;
  testId?: string;
}

type Mark =
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "code"
  | "bulletList"
  | "orderedList"
  | "blockquote"
  | "link";

const TOOLS: readonly { id: Mark; icon: LucideIcon; run: (editor: Editor) => void }[] = [
  { id: "bold", icon: Bold, run: (e) => e.chain().focus().toggleBold().run() },
  { id: "italic", icon: Italic, run: (e) => e.chain().focus().toggleItalic().run() },
  { id: "underline", icon: Underline, run: (e) => e.chain().focus().toggleUnderline().run() },
  { id: "strike", icon: Strikethrough, run: (e) => e.chain().focus().toggleStrike().run() },
  { id: "code", icon: Code, run: (e) => e.chain().focus().toggleCode().run() },
  { id: "bulletList", icon: List, run: (e) => e.chain().focus().toggleBulletList().run() },
  { id: "orderedList", icon: ListOrdered, run: (e) => e.chain().focus().toggleOrderedList().run() },
  { id: "blockquote", icon: TextQuote, run: (e) => e.chain().focus().toggleBlockquote().run() },
];

const ALIGNMENTS: readonly { id: Alignment; icon: LucideIcon }[] = [
  { id: "left", icon: AlignLeft },
  { id: "center", icon: AlignCenter },
  { id: "right", icon: AlignRight },
  { id: "justify", icon: AlignJustify },
];

/** The block styles the core keeps: paragraphs, headings 1 to 3 and code blocks. */
export const BLOCK_STYLES = ["paragraph", "heading1", "heading2", "heading3", "codeBlock"] as const;
export type BlockStyle = (typeof BLOCK_STYLES)[number];

function blockStyleOf(editor: Editor): BlockStyle {
  for (const level of [1, 2, 3] as const) if (editor.isActive("heading", { level })) return `heading${String(level)}` as BlockStyle;
  if (editor.isActive("codeBlock")) return "codeBlock";
  return "paragraph";
}

function setBlockStyle(editor: Editor, style: BlockStyle): void {
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

function alignmentOf(editor: Editor): Alignment {
  for (const align of ["center", "right", "justify"] as const) {
    if (editor.isActive({ textAlign: align })) return align;
  }
  return "left";
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

/**
 * The rich text body (TipTap) with what the core's allowlist keeps (decision 0023): bold,
 * italic, underline, strikethrough, inline code, code blocks, headings 1 to 3, lists, quotes,
 * links, alignment, `cid:` images, plus undo and redo. TipTap's own stylesheet is off
 * (`injectCSS: false`): the CSP allows no injected `<style>`; `composer.css` has the rules.
 *
 * Pasted HTML (Word, web pages) goes through the core's `cleanPastedHtml` before it enters the
 * editor. Cmd/Ctrl+Enter is "send": the editor must not turn it into a line break.
 */
export function RichEditor({
  initialHtml,
  onChange,
  onBlur,
  label,
  disabled = false,
  autoFocus = false,
  imageSources,
  onInsertImage,
  handleRef,
  onPasteResult,
  testId = "composer-rich-body",
}: RichEditorProps) {
  const { t } = useTranslation();
  const change = useRef(onChange);
  const blur = useRef(onBlur);
  const pasted = useRef(onPasteResult);
  useEffect(() => {
    change.current = onChange;
    blur.current = onBlur;
    pasted.current = onPasteResult;
  });
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkValue, setLinkValue] = useState("");
  const [linkError, setLinkError] = useState(false);
  const linkId = useId();
  const linkErrorId = useId();
  const blockId = useId();
  const sizeId = useId();

  // Inline images look up their `data:` URL here; the node views follow changes of the map.
  const inlineImage = useMemo(() => createInlineImage(), []);
  useEffect(() => {
    inlineImage.setSources(imageSources ?? {});
  }, [inlineImage, imageSources]);

  const extensions = useMemo(
    () => [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        horizontalRule: false,
        // No empty paragraph after a heading or code block: it would go out as an empty line.
        trailingNode: false,
        link: { openOnClick: false, autolink: true, linkOnPaste: true, protocols: ["mailto"] },
      }),
      TextAlign,
      inlineImage.extension,
      Extension.create({
        name: "composerSubmit",
        priority: 1000,
        addKeyboardShortcuts: () => ({ "Mod-Enter": () => true }),
      }),
    ],
    [inlineImage],
  );

  const editor = useEditor({
    extensions,
    content: initialHtml,
    injectCSS: false,
    immediatelyRender: true,
    autofocus: autoFocus ? "start" : false,
    editable: !disabled,
    editorProps: {
      attributes: {
        "aria-label": label,
        "aria-multiline": "true",
        role: "textbox",
        // The system's spell checker (WKWebView, WebView2), in the languages the system offers.
        spellcheck: "true",
        class: "composer-prose px-4 py-3 text-14 text-primary outline-none",
        "data-testid": testId,
      },
      handlePaste: (view, event) => {
        const html = event.clipboardData?.getData("text/html") ?? "";
        if (html.trim() === "") return false; // plain text: ProseMirror inserts it as text
        const text = event.clipboardData?.getData("text/plain") ?? "";
        event.preventDefault();
        void cleanPaste(html, text).then(
          (outcome) => {
            if (view.isDestroyed) return;
            if (outcome.kind === "cleaned") {
              const current = editorRef.current;
              if (current) current.chain().focus().insertContent(outcome.html).run();
            } else {
              view.dispatch(view.state.tr.insertText(outcome.text));
            }
            pasted.current?.(outcome);
          },
          (e: unknown) => {
            pasted.current?.({ kind: "failed", error: e as CoreError });
          },
        );
        return true;
      },
      handleDrop: (_view, event) => {
        // Files dropped on the window are the shell's (attachments, inline images); HTML dragged
        // in from elsewhere is not cleaned by the core, so the editor does not take it.
        const types = Array.from(event.dataTransfer?.types ?? []);
        return types.includes("Files") || types.includes("text/html");
      },
    },
    onUpdate: ({ editor: e }) => {
      change.current(e.isEmpty ? "" : e.getHTML());
    },
    onBlur: () => {
      blur.current();
    },
  });
  const editorRef = useRef<Editor | null>(null);
  useEffect(() => {
    editorRef.current = editor;
  }, [editor]);

  useEffect(() => {
    editor.setEditable(!disabled);
  }, [editor, disabled]);

  useImperativeHandle(
    handleRef,
    () => ({
      insertHtml: (html) => {
        editor.chain().focus().insertContent(html).run();
      },
      insertImages: (images) => {
        if (images.length === 0) return;
        editor
          .chain()
          .focus()
          .insertContent(images.map((image) => ({ type: "inlineImage", attrs: { src: `cid:${image.contentId}`, alt: image.alt ?? "" } })))
          .run();
      },
      focus: () => {
        editor.commands.focus();
      },
    }),
    [editor],
  );

  const active = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      bold: e.isActive("bold"),
      italic: e.isActive("italic"),
      underline: e.isActive("underline"),
      strike: e.isActive("strike"),
      code: e.isActive("code"),
      bulletList: e.isActive("bulletList"),
      orderedList: e.isActive("orderedList"),
      blockquote: e.isActive("blockquote"),
      link: e.isActive("link"),
      block: blockStyleOf(e),
      align: alignmentOf(e),
      canUndo: e.can().undo(),
      canRedo: e.can().redo(),
      image: e.isActive("inlineImage") ? imageSizeOf(e.getAttributes("inlineImage")["width"]) : null,
    }),
  });

  const openLink = () => {
    const current = editor.getAttributes("link")["href"] as string | undefined;
    setLinkValue(current ?? "");
    setLinkError(false);
    setLinkOpen(true);
  };
  const applyLink = () => {
    if (linkValue.trim() === "") {
      editor.chain().focus().extendMarkRange("link").unsetLink().run();
      setLinkOpen(false);
      return;
    }
    const href = normaliseLink(linkValue);
    if (!href) {
      setLinkError(true);
      return;
    }
    editor.chain().focus().extendMarkRange("link").setLink({ href }).run();
    setLinkOpen(false);
  };
  const onLinkKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      applyLink();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setLinkOpen(false);
      editor.commands.focus();
    }
  };
  const setAlignment = (align: Alignment) => {
    const value = align === "left" ? null : align;
    editor.chain().focus().updateAttributes("paragraph", { textAlign: value }).updateAttributes("heading", { textAlign: value }).run();
  };
  const setImageSize = (size: ImageSize) => {
    editor.chain().focus().updateAttributes("inlineImage", { width: IMAGE_SIZES[size] }).run();
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        role="toolbar"
        aria-label={t("composer.format.toolbar")}
        className="flex shrink-0 flex-wrap items-center gap-1 border-b border-border px-3 py-1"
        data-testid="composer-format-toolbar"
      >
        <FormatButton
          icon={Undo2}
          label={t("writing.format.undo")}
          disabled={disabled || !active?.canUndo}
          onClick={() => editor.chain().focus().undo().run()}
          data-testid="format-undo"
        />
        <FormatButton
          icon={Redo2}
          label={t("writing.format.redo")}
          disabled={disabled || !active?.canRedo}
          onClick={() => editor.chain().focus().redo().run()}
          data-testid="format-redo"
        />
        <label htmlFor={blockId} className="sr-only">
          {t("writing.format.blockStyle")}
        </label>
        <select
          id={blockId}
          value={active?.block ?? BLOCK_STYLES[0]}
          disabled={disabled}
          onChange={(e) => {
            const style = BLOCK_STYLES.find((s) => s === e.target.value);
            if (style) setBlockStyle(editor, style);
          }}
          className="h-8 rounded-control bg-transparent px-1 text-13 text-primary hover:bg-hover"
          data-testid="format-block"
        >
          {BLOCK_STYLES.map((style) => (
            <option key={style} value={style}>
              {t(`writing.format.block.${style}`)}
            </option>
          ))}
        </select>
        {TOOLS.map((tool) => (
          <FormatButton
            key={tool.id}
            icon={tool.icon}
            label={t(tool.id === "strike" || tool.id === "code" ? `writing.format.${tool.id}` : `composer.format.${tool.id}`)}
            pressed={active?.[tool.id] ?? false}
            disabled={disabled}
            onClick={() => {
              tool.run(editor);
            }}
            data-testid={`format-${tool.id}`}
          />
        ))}
        <FormatButton
          icon={Link2}
          label={t("composer.format.link")}
          pressed={active?.link ?? false}
          disabled={disabled}
          onClick={openLink}
          data-testid="format-link"
        />
        <span role="group" aria-label={t("writing.format.alignment")} className="flex items-center gap-1">
          {ALIGNMENTS.map((alignment) => (
            <FormatButton
              key={alignment.id}
              icon={alignment.icon}
              label={t(`writing.format.align.${alignment.id}`)}
              pressed={(active?.align ?? "left") === alignment.id}
              disabled={disabled}
              onClick={() => {
                setAlignment(alignment.id);
              }}
              data-testid={`format-align-${alignment.id}`}
            />
          ))}
        </span>
        {onInsertImage && (
          <FormatButton
            icon={ImagePlus}
            label={t("writing.images.insert")}
            disabled={disabled}
            onClick={onInsertImage}
            data-testid="format-image"
          />
        )}
        {active?.image && (
          <span className="flex items-center gap-1">
            <label htmlFor={sizeId} className="text-12 text-secondary">
              {t("writing.images.size")}
            </label>
            <select
              id={sizeId}
              value={active.image}
              disabled={disabled}
              onChange={(e) => {
                const size = (Object.keys(IMAGE_SIZES) as ImageSize[]).find((s) => s === e.target.value);
                if (size) setImageSize(size);
              }}
              className="h-8 rounded-control bg-transparent px-1 text-13 text-primary hover:bg-hover"
              data-testid="format-image-size"
            >
              {(Object.keys(IMAGE_SIZES) as ImageSize[]).map((size) => (
                <option key={size} value={size}>
                  {t(`writing.images.sizes.${size}`)}
                </option>
              ))}
            </select>
          </span>
        )}
      </div>
      {linkOpen && (
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-4 py-2" data-testid="composer-link-editor">
          <label htmlFor={linkId} className="text-13 text-secondary">
            {t("composer.format.linkAddress")}
          </label>
          <input
            id={linkId}
            autoFocus
            value={linkValue}
            onChange={(e) => {
              setLinkValue(e.target.value);
              setLinkError(false);
            }}
            onKeyDown={onLinkKey}
            aria-invalid={linkError || undefined}
            aria-describedby={linkError ? linkErrorId : undefined}
            className={cx(
              "h-8 min-w-0 flex-1 rounded-control border bg-surface px-2 text-13 text-primary",
              linkError ? "border-danger" : "border-border-strong",
            )}
            data-testid="composer-link-input"
          />
          <Button variant="secondary" onClick={applyLink} data-testid="composer-link-apply">
            {linkValue.trim() === "" ? t("composer.format.linkRemove") : t("composer.format.linkApply")}
          </Button>
          {linkError && (
            <p id={linkErrorId} className="w-full text-12 text-danger">
              {t("composer.format.linkInvalid")}
            </p>
          )}
        </div>
      )}
      <EditorContent editor={editor} className="min-h-0 flex-1 overflow-y-auto" />
    </div>
  );
}

interface FormatButtonProps {
  icon: LucideIcon;
  label: string;
  pressed?: boolean;
  disabled: boolean;
  onClick: () => void;
  "data-testid": string;
}

function FormatButton({ icon: Icon, label, pressed, disabled, onClick, ...rest }: FormatButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={pressed}
      disabled={disabled}
      // Keep the selection in the editor: the button must not take the focus on mouse down.
      onMouseDown={(e) => {
        e.preventDefault();
      }}
      onClick={onClick}
      className={cx(
        "flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50",
        pressed && "bg-selected text-primary",
      )}
      data-testid={rest["data-testid"]}
    >
      <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
    </button>
  );
}
