import { Folder, Tag, X, type LucideIcon } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ComboBox, type ComboOption } from "../../components/ComboBox";
import { IconButton } from "../../components/IconButton";
import type { Mailbox, MessageListItem } from "../../services/ipc";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { buildMailboxTree, isRoleView, type MailboxNode } from "../mailboxes/tree";
import { hasTag, isValidKeyword, tagsOf } from "./keywords";
import type { ApplyOutcome } from "./useActionController";

interface FloatingPanelProps {
  title: string;
  /** Escape, the close button, a click or the focus outside. */
  onClose: () => void;
  /** Where the focus goes if the element that had it before is gone (the list). */
  restoreFocus: () => void;
  children: ReactNode;
  "data-testid"?: string;
}

/**
 * A small floating panel at the top of the mail view for choosing (folder, tags). Not modal and
 * no dialog for everyday things (docs/DESIGN.md): it closes as soon as the user turns elsewhere,
 * and gives the focus back to where it came from.
 */
export function FloatingPanel({ title, onClose, restoreFocus, children, ...rest }: FloatingPanelProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const [opener] = useState(() => (document.activeElement instanceof HTMLElement ? document.activeElement : null));
  const close = useRef(onClose);
  const restore = useRef(restoreFocus);
  useEffect(() => {
    close.current = onClose;
    restore.current = restoreFocus;
  });

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !panelRef.current?.contains(event.target)) close.current();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      // Back to where the panel was opened from, unless an action moved the focus already.
      const active = document.activeElement;
      if (active && active !== document.body) return;
      if (opener?.isConnected && opener !== document.body) opener.focus({ preventScroll: true });
      else restore.current();
    };
  }, [opener]);

  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-labelledby={titleId}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
      onBlur={(event) => {
        if (event.relatedTarget instanceof Node && !event.currentTarget.contains(event.relatedTarget)) onClose();
      }}
      data-testid={rest["data-testid"]}
      className="fixed top-12 left-1/2 z-40 flex max-h-full w-(--ui-picker-width) max-w-full -translate-x-1/2 flex-col gap-3 rounded-panel border border-border bg-surface-raised p-4 shadow-floating"
    >
      <div className="flex items-center justify-between gap-2">
        <h2 id={titleId} className="truncate text-14 font-semibold text-primary">
          {title}
        </h2>
        <IconButton icon={X} label={t("actions.picker.close")} onClick={onClose} />
      </div>
      {children}
    </div>
  );
}

function flatten(nodes: readonly MailboxNode[]): Mailbox[] {
  return nodes.flatMap((node) => [node.mailbox, ...flatten(node.children)]);
}

function folderIcon(mailbox: Mailbox): LucideIcon {
  return isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder;
}

/** Folders to choose from, in the order of the folder tree, as combobox options. */
export function useFolderOptions(mailboxes: readonly Mailbox[], query: string, exclude: (mailbox: Mailbox) => boolean) {
  const { t } = useTranslation();
  const ordered = useMemo(() => flatten(buildMailboxTree(mailboxes)), [mailboxes]);
  const needle = query.trim().toLowerCase();
  return ordered
    .filter((mailbox) => mailbox.selectable && !exclude(mailbox))
    .map((mailbox): ComboOption<Mailbox> => {
      const label = mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`);
      const Icon = folderIcon(mailbox);
      return {
        key: String(mailbox.id),
        label,
        ...(mailbox.path !== label && { detail: mailbox.path }),
        icon: <Icon aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />,
        value: mailbox,
      };
    })
    .filter((option) => !needle || option.label.toLowerCase().includes(needle) || option.detail?.toLowerCase().includes(needle));
}

interface MovePickerProps {
  count: number;
  mailboxes: readonly Mailbox[];
  /** The folder the messages are in; not offered. */
  current: Mailbox;
  onChoose: (target: Mailbox) => void;
  onClose: () => void;
  restoreFocus: () => void;
}

/** "Move to …": the account's folders with a filter, keyboard first (shortcut V). */
export function MovePicker({ count, mailboxes, current, onChoose, onClose, restoreFocus }: MovePickerProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const options = useFolderOptions(mailboxes, query, (mailbox) => mailbox.id === current.id);
  const others = mailboxes.some((mailbox) => mailbox.selectable && mailbox.id !== current.id);
  return (
    <FloatingPanel title={t("actions.move.title", { count })} onClose={onClose} restoreFocus={restoreFocus} data-testid="move-picker">
      <ComboBox
        label={t("actions.picker.filter")}
        query={query}
        onQueryChange={setQuery}
        options={options}
        onChoose={(option) => {
          onChoose(option.value);
        }}
        emptyText={others ? t("actions.picker.empty", { query: query.trim() }) : t("actions.picker.noFolders")}
        autoFocus
        data-testid="move-chooser"
      />
    </FloatingPanel>
  );
}

interface TagEditorProps {
  /** The messages the tags are for, as they are now (they change after each action). */
  items: readonly MessageListItem[];
  /** Every loaded message of the folder, for suggestions. */
  known: readonly MessageListItem[];
  onAdd: (tag: string) => Promise<ApplyOutcome>;
  onRemove: (tag: string) => Promise<ApplyOutcome>;
  onClose: () => void;
  restoreFocus: () => void;
}

/**
 * Tags (IMAP keywords) of the selected messages: the ones they carry with a remove button each,
 * and a field to add one, with the existing tags of the folder as suggestions. Characters IMAP
 * does not allow are explained at the field before anything is sent.
 */
export function TagEditor({ items, known, onAdd, onRemove, onClose, restoreFocus }: TagEditorProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [invalid, setInvalid] = useState(false);
  const [busy, setBusy] = useState(false);
  const current = tagsOf(items);
  const typed = query.trim();
  const needle = typed.toLowerCase();
  const onAll = (tag: string) => items.length > 0 && items.every((item) => hasTag(item, tag));
  const suggestions = tagsOf(known).filter((tag) => !onAll(tag) && (!needle || tag.toLowerCase().includes(needle)));
  const exact = suggestions.some((tag) => tag.toLowerCase() === needle) || (typed !== "" && onAll(typed));

  const options: ComboOption<string>[] = [
    ...(typed && !exact ? [{ key: `new:${typed}`, label: t("actions.tags.add", { tag: typed }), value: typed }] : []),
    ...suggestions.map((tag) => ({
      key: `tag:${tag}`,
      label: tag,
      icon: <Tag aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />,
      value: tag,
    })),
  ];

  const add = async (tag: string) => {
    if (!isValidKeyword(tag)) {
      setInvalid(true);
      return;
    }
    setBusy(true);
    const outcome = await onAdd(tag);
    setBusy(false);
    if (outcome.ok) {
      setQuery("");
      setInvalid(false);
    } else if (outcome.error?.code === "invalid_keyword") {
      setInvalid(true);
    }
  };

  return (
    <FloatingPanel title={t("actions.tags.title", { count: items.length })} onClose={onClose} restoreFocus={restoreFocus} data-testid="tag-editor">
      <section aria-label={t("actions.tags.current")} className="flex flex-col gap-2">
        {current.length === 0 ? (
          <p className="text-13 text-secondary">{t("actions.tags.none")}</p>
        ) : (
          <ul className="flex flex-wrap gap-2">
            {current.map((tag) => (
              <li key={tag} className="flex items-center gap-1 rounded-control border border-border-strong ps-2 text-13 text-primary">
                <span className="select-text">{tag}</span>
                <button
                  type="button"
                  aria-label={t("actions.tags.remove", { tag })}
                  disabled={busy}
                  onClick={() => {
                    void onRemove(tag);
                  }}
                  className="flex size-6 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
                  data-testid={`remove-tag-${tag}`}
                >
                  <X aria-hidden className="size-3" strokeWidth={2} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <ComboBox
        label={t("actions.tags.field")}
        query={query}
        onQueryChange={(next) => {
          setQuery(next);
          if (invalid) setInvalid(false);
        }}
        options={options}
        onChoose={(option) => {
          if (!busy) void add(option.value);
        }}
        emptyText={t("actions.tags.noSuggestions")}
        description={t("actions.tags.description")}
        error={invalid ? t("actions.tags.invalid") : null}
        listLabel={t("actions.tags.suggestions")}
        autoFocus
        data-testid="tag-field"
      />
    </FloatingPanel>
  );
}
