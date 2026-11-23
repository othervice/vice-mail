import { EditorContent, Extension, useEditor, useEditorState, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Bold, Italic, Link2, List, ListOrdered, TextQuote, Underline, type LucideIcon } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import "./composer.css";

interface RichEditorProps {
  /** Editor HTML at the start; later changes come from the user only. */
  initialHtml: string;
  onChange: (html: string) => void;
  onBlur: () => void;
  label: string;
  disabled?: boolean;
  /** Replies start in the body, above the quote. */
  autoFocus?: boolean;
}

/**
 * Only what the core's allowlist keeps (strong, em, u, a, lists, blockquote, paragraphs, breaks).
 * Cmd/Ctrl+Enter is "send": the editor must not turn it into a line break; the key event goes on
 * to the composer, which sends.
 */
const EXTENSIONS = [
  StarterKit.configure({
    heading: false,
    code: false,
    codeBlock: false,
    horizontalRule: false,
    strike: false,
    link: { openOnClick: false, autolink: true, linkOnPaste: true, protocols: ["mailto"] },
  }),
  Extension.create({
    name: "composerSubmit",
    priority: 1000,
    addKeyboardShortcuts: () => ({ "Mod-Enter": () => true }),
  }),
];

type Mark = "bold" | "italic" | "underline" | "bulletList" | "orderedList" | "blockquote" | "link";

const TOOLS: readonly { id: Mark; icon: LucideIcon; testId: string; run: (editor: Editor) => void }[] = [
  { id: "bold", icon: Bold, testId: "format-bold", run: (e) => e.chain().focus().toggleBold().run() },
  { id: "italic", icon: Italic, testId: "format-italic", run: (e) => e.chain().focus().toggleItalic().run() },
  { id: "underline", icon: Underline, testId: "format-underline", run: (e) => e.chain().focus().toggleUnderline().run() },
  { id: "bulletList", icon: List, testId: "format-bulletList", run: (e) => e.chain().focus().toggleBulletList().run() },
  { id: "orderedList", icon: ListOrdered, testId: "format-orderedList", run: (e) => e.chain().focus().toggleOrderedList().run() },
  { id: "blockquote", icon: TextQuote, testId: "format-blockquote", run: (e) => e.chain().focus().toggleBlockquote().run() },
];

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
 * The rich text body (TipTap): bold, italic, underline, lists, quote and links, nothing else.
 * TipTap's own stylesheet is off (`injectCSS: false`): the CSP allows no injected `<style>`;
 * `composer.css` has the few rules ProseMirror needs.
 */
export function RichEditor({ initialHtml, onChange, onBlur, label, disabled = false, autoFocus = false }: RichEditorProps) {
  const { t } = useTranslation();
  const change = useRef(onChange);
  const blur = useRef(onBlur);
  useEffect(() => {
    change.current = onChange;
    blur.current = onBlur;
  });
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkValue, setLinkValue] = useState("");
  const [linkError, setLinkError] = useState(false);
  const linkId = useId();
  const linkErrorId = useId();

  const editor = useEditor({
    extensions: EXTENSIONS,
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
        class: "composer-prose px-4 py-3 text-14 text-primary outline-none",
        "data-testid": "composer-rich-body",
      },
    },
    onUpdate: ({ editor: e }) => {
      change.current(e.isEmpty ? "" : e.getHTML());
    },
    onBlur: () => {
      blur.current();
    },
  });

  useEffect(() => {
    editor.setEditable(!disabled);
  }, [editor, disabled]);

  const active = useEditorState({
    editor,
    selector: ({ editor: e }): Record<Mark, boolean> => ({
      bold: e.isActive("bold"),
      italic: e.isActive("italic"),
      underline: e.isActive("underline"),
      bulletList: e.isActive("bulletList"),
      orderedList: e.isActive("orderedList"),
      blockquote: e.isActive("blockquote"),
      link: e.isActive("link"),
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

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        role="toolbar"
        aria-label={t("composer.format.toolbar")}
        className="flex shrink-0 flex-wrap items-center gap-1 border-b border-border px-3 py-1"
        data-testid="composer-format-toolbar"
      >
        {TOOLS.map((tool) => (
          <FormatButton
            key={tool.id}
            icon={tool.icon}
            label={t(`composer.format.${tool.id}`)}
            pressed={active?.[tool.id] ?? false}
            disabled={disabled}
            onClick={() => {
              tool.run(editor);
            }}
            data-testid={tool.testId}
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
  pressed: boolean;
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
