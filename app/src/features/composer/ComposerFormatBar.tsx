import { useEditorState, type Editor } from "@tiptap/react";
import {
  Bold,
  ChevronDown,
  EllipsisVertical,
  ImagePlus,
  IndentDecrease,
  IndentIncrease,
  Italic,
  Link2,
  List,
  Redo2,
  RemoveFormatting,
  TextQuote,
  Underline,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import { useId, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { ComposerMenu, type MenuEntry, type MenuSection } from "./ComposerMenu";
import { alignmentOf, BLOCK_STYLES, blockStyleOf, clearFormatting, normaliseLink, setAlignment, setBlockStyle } from "./formatting";
import { IMAGE_SIZES, imageSizeOf, type ImageSize } from "./inlineImage";

interface ComposerFormatBarProps {
  editor: Editor;
  disabled: boolean;
  /** "Insert image" (the core's file dialog). */
  onInsertImage?: (() => void) | undefined;
  /** "Plain text": the draft leaves rich text (the composer converts the body). */
  onPlainText?: (() => void) | undefined;
}

/** A button of the bar's chip group: 24 px high, on the chip's tone. */
const BAR_BUTTON = "flex h-6 items-center justify-center rounded-control text-secondary transition-colors hover:bg-surface-raised hover:text-primary disabled:opacity-50";
const BAR_ICON = "w-8";
const BAR_MENU_TEXT = "gap-1 px-2 text-12";
const BAR_OPEN = "bg-surface-raised text-primary";

/**
 * Stage 23 (decision 0086): the composer's compact formatting bar, at its foot above the send
 * row, a rounded group on the chip tone: paragraph style, bold, italic, underline, list, link,
 * quote, and "More formatting" with everything else the full toolbar had (strikethrough, code,
 * numbered list, alignment, undo and redo, inline image, image size) and what it lacked
 * (indent and outdent of list items, clear formatting), plus the switch to plain text. While an
 * inline image is selected, its size is a menu of its own in the group. Every button keeps the
 * editor's selection (no focus on mouse down); the shortcuts of the editor (⌘B, ⌘I, ⌘U, ⌘Z)
 * work as before.
 */
export function ComposerFormatBar({ editor, disabled, onInsertImage, onPlainText }: ComposerFormatBarProps) {
  const { t } = useTranslation();
  const linkId = useId();
  const linkErrorId = useId();
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkValue, setLinkValue] = useState("");
  const [linkError, setLinkError] = useState(false);
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
      canIndent: e.can().sinkListItem("listItem"),
      canOutdent: e.can().liftListItem("listItem"),
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
      // The link field's, not the composer's.
      event.preventDefault();
      event.stopPropagation();
      setLinkOpen(false);
      editor.commands.focus();
    }
  };

  const block = active?.block ?? BLOCK_STYLES[0];
  const blockLabel = t(`writing.format.block.${block}`);
  const blockMenu: MenuSection[] = [
    {
      id: "block",
      entries: BLOCK_STYLES.map(
        (style): MenuEntry => ({
          kind: "radio",
          id: style,
          label: t(`writing.format.block.${style}`),
          checked: block === style,
          onSelect: () => {
            setBlockStyle(editor, style);
          },
          "data-testid": `format-block-${style}`,
        }),
      ),
    },
  ];
  const toggle = (id: "strike" | "code" | "orderedList", label: string, run: () => void): MenuEntry => ({
    kind: "checkbox",
    id,
    label,
    checked: active?.[id] ?? false,
    closes: true,
    onSelect: run,
    "data-testid": `format-${id}`,
  });
  const align = active?.align ?? "left";
  const moreMenu: MenuSection[] = [
    {
      id: "marks",
      entries: [
        toggle("strike", t("writing.format.strike"), () => editor.chain().focus().toggleStrike().run()),
        toggle("code", t("writing.format.code"), () => editor.chain().focus().toggleCode().run()),
        toggle("orderedList", t("composer.format.orderedList"), () => editor.chain().focus().toggleOrderedList().run()),
      ],
    },
    {
      id: "align",
      label: t("writing.format.alignment"),
      entries: (["left", "center", "right", "justify"] as const).map(
        (id): MenuEntry => ({
          kind: "radio",
          id,
          label: t(`writing.format.align.${id}`),
          checked: align === id,
          onSelect: () => {
            setAlignment(editor, id);
          },
          "data-testid": `format-align-${id}`,
        }),
      ),
    },
    {
      id: "indent",
      entries: [
        {
          kind: "action",
          id: "indent",
          label: t("composer.format.indent"),
          icon: IndentIncrease,
          disabled: !active?.canIndent,
          onSelect: () => editor.chain().focus().sinkListItem("listItem").run(),
          "data-testid": "format-indent",
        },
        {
          kind: "action",
          id: "outdent",
          label: t("composer.format.outdent"),
          icon: IndentDecrease,
          disabled: !active?.canOutdent,
          onSelect: () => editor.chain().focus().liftListItem("listItem").run(),
          "data-testid": "format-outdent",
        },
      ],
    },
    {
      id: "edit",
      entries: [
        ...(onInsertImage
          ? [{ kind: "action", id: "image", label: t("writing.images.insert"), icon: ImagePlus, onSelect: onInsertImage, "data-testid": "format-image" } satisfies MenuEntry]
          : []),
        {
          kind: "action",
          id: "clear",
          label: t("composer.format.clear"),
          icon: RemoveFormatting,
          onSelect: () => {
            clearFormatting(editor);
          },
          "data-testid": "format-clear",
        },
        {
          kind: "action",
          id: "undo",
          label: t("writing.format.undo"),
          icon: Undo2,
          disabled: !active?.canUndo,
          onSelect: () => editor.chain().focus().undo().run(),
          "data-testid": "format-undo",
        },
        {
          kind: "action",
          id: "redo",
          label: t("writing.format.redo"),
          icon: Redo2,
          disabled: !active?.canRedo,
          onSelect: () => editor.chain().focus().redo().run(),
          "data-testid": "format-redo",
        },
      ],
    },
    ...(onPlainText
      ? [
          {
            id: "format",
            label: t("composer.format.label"),
            entries: [
              { kind: "radio", id: "html", label: t("composer.format.html"), checked: true, onSelect: () => undefined, "data-testid": "format-menu-html" },
              { kind: "radio", id: "text", label: t("composer.format.text"), checked: false, onSelect: onPlainText, "data-testid": "format-menu-text" },
            ],
          } satisfies MenuSection,
        ]
      : []),
  ];
  const imageMenu: MenuSection[] = [
    {
      id: "size",
      entries: (Object.keys(IMAGE_SIZES) as ImageSize[]).map(
        (size): MenuEntry => ({
          kind: "radio",
          id: size,
          label: t(`writing.images.sizes.${size}`),
          checked: active?.image === size,
          onSelect: () => editor.chain().focus().updateAttributes("inlineImage", { width: IMAGE_SIZES[size] }).run(),
          "data-testid": `format-image-size-${size}`,
        }),
      ),
    },
  ];

  // A choice in these menus acts on the text: the editor takes the focus back first (at once,
  // not on the next frame), so its selection and the marks to type with are the ones it had.
  const inEditor = (sections: readonly MenuSection[]): MenuSection[] =>
    sections.map((section) => ({
      ...section,
      entries: section.entries.map((entry) => ({
        ...entry,
        onSelect: () => {
          editor.view.focus();
          entry.onSelect();
        },
      })),
    }));

  return (
    <div className="flex min-w-0 flex-col gap-2" data-testid="composer-format-bar">
      {linkOpen && (
        <div className="flex flex-wrap items-center gap-2" data-testid="composer-link-editor">
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
      <div
        role="toolbar"
        aria-label={t("composer.format.toolbar")}
        className="flex w-fit max-w-full flex-wrap items-center gap-1 rounded-tile bg-badge p-1"
        data-testid="composer-format-toolbar"
      >
        <ComposerMenu
          label={t("writing.format.blockStyle")}
          name={t("composer.format.blockStyleNamed", { style: blockLabel })}
          sections={inEditor(blockMenu)}
          disabled={disabled}
          tooltip={false}
          className={(open) => cx(BAR_BUTTON, BAR_MENU_TEXT, open && BAR_OPEN)}
          refocus={false}
          data-testid="format-block"
        >
          <span>{blockLabel}</span>
          <ChevronDown aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </ComposerMenu>
        <Separator />
        <BarButton icon={Bold} label={t("composer.format.bold")} pressed={active?.bold ?? false} disabled={disabled} onClick={() => editor.chain().focus().toggleBold().run()} data-testid="format-bold" />
        <BarButton icon={Italic} label={t("composer.format.italic")} pressed={active?.italic ?? false} disabled={disabled} onClick={() => editor.chain().focus().toggleItalic().run()} data-testid="format-italic" />
        <BarButton
          icon={Underline}
          label={t("composer.format.underline")}
          pressed={active?.underline ?? false}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleUnderline().run()}
          data-testid="format-underline"
        />
        <Separator />
        <BarButton
          icon={List}
          label={t("composer.format.bulletList")}
          pressed={active?.bulletList ?? false}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleBulletList().run()}
          data-testid="format-bulletList"
        />
        <BarButton icon={Link2} label={t("composer.format.link")} pressed={active?.link ?? false} disabled={disabled} onClick={openLink} data-testid="format-link" />
        <BarButton
          icon={TextQuote}
          label={t("composer.format.blockquote")}
          pressed={active?.blockquote ?? false}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleBlockquote().run()}
          data-testid="format-blockquote"
        />
        {active?.image && (
          <ComposerMenu
            label={t("writing.images.size")}
            name={t("composer.format.imageSizeNamed", { size: t(`writing.images.sizes.${active.image}`) })}
            sections={inEditor(imageMenu)}
            disabled={disabled}
            tooltip={false}
            className={(open) => cx(BAR_BUTTON, BAR_MENU_TEXT, open && BAR_OPEN)}
            refocus={false}
            data-testid="format-image-size"
          >
            <span>{t(`writing.images.sizes.${active.image}`)}</span>
            <ChevronDown aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          </ComposerMenu>
        )}
        <Separator />
        <ComposerMenu
          label={t("composer.format.more")}
          sections={inEditor(moreMenu)}
          disabled={disabled}
          className={(open) => cx(BAR_BUTTON, BAR_ICON, open && BAR_OPEN)}
          refocus={false}
          data-testid="format-more"
        >
          <EllipsisVertical aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </ComposerMenu>
      </div>
    </div>
  );
}

function Separator() {
  return <span aria-hidden className="mx-1 h-4 w-px bg-border" />;
}

interface BarButtonProps {
  icon: LucideIcon;
  label: string;
  pressed: boolean;
  disabled: boolean;
  onClick: () => void;
  "data-testid": string;
}

function BarButton({ icon: Icon, label, pressed, disabled, onClick, ...rest }: BarButtonProps) {
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-label={label}
        aria-pressed={pressed}
        disabled={disabled}
        // Keep the selection in the editor: the button must not take the focus on mouse down.
        onMouseDown={(e) => {
          e.preventDefault();
        }}
        onClick={onClick}
        className={cx(BAR_BUTTON, BAR_ICON, pressed && "bg-surface-raised text-accent")}
        data-testid={rest["data-testid"]}
      >
        <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
