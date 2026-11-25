import { useInfiniteQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Download, ExternalLink, Eye, LoaderCircle, Mail, Paperclip, Search } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import {
  attachmentsViewIpc,
  type AttachmentCursor,
  type AttachmentEntry,
  type AttachmentFilter,
  type AttachmentKind,
  type AttachmentPage,
  type IpcError,
  type MarkScope,
} from "../../services/ipc";
import { selectWhenListed } from "../../stores/listSelect";
import { useUiStore } from "../../stores/ui";
import { useAccounts } from "../accounts/queries";
import { formatFullDate } from "../messages/rows";
import { openMessageInWindow } from "../windows/open";
import { AttachmentPreviewDialog } from "./AttachmentPreview";
import { ATTACHMENT_ICONS, attachmentKind, fileType, formatSize } from "./format";
import { useOpenAttachment, useSaveAttachment } from "./queries";

/** Attachments per page asked from the core (it allows up to 500). */
export const ATTACHMENTS_PAGE_SIZE = 200;

const KINDS: readonly AttachmentKind[] = ["image", "pdf", "document", "spreadsheet", "archive", "other"];

/** Where the query keys of the view start; `messagesChanged` and new bodies reload them. */
export const ATTACHMENTS_VIEW_KEY = "attachmentsView";

type Feedback = { kind: "saved"; name: string; path: string } | { kind: "error"; context: string; error: IpcError };

/** Shows the attachment's message where mail is read: selected in its folder's list. */
function goToMessage(entry: AttachmentEntry): void {
  const { accountId, mailboxId, messageId } = entry;
  useUiStore.getState().showMailbox(accountId, mailboxId);
  selectWhenListed({
    accountId,
    mailboxId,
    messageId,
    fallback: () => {
      openMessageInWindow(accountId, messageId);
    },
  });
}

const rowKey = (entry: AttachmentEntry) => `${String(entry.messageId)}/${String(entry.index)}`;

interface RowProps {
  entry: AttachmentEntry;
  onPreview: () => void;
  onFeedback: (feedback: Feedback | null) => void;
  onNeedsConfirmation: (entry: AttachmentEntry) => void;
}

function AttachmentRow({ entry, onPreview, onFeedback, onNeedsConfirmation }: RowProps) {
  const { t, i18n } = useTranslation();
  const owner = { messageId: entry.messageId };
  const save = useSaveAttachment(owner, entry.index);
  const open = useOpenAttachment(owner, entry.index);
  const Icon = ATTACHMENT_ICONS[attachmentKind(entry.mimeType, entry.executable)];
  const sender = entry.from ? entry.from.name?.trim() || entry.from.email : t("attachments.view.unknownSender");
  const size = formatSize(entry.size, i18n.language);
  const busy = save.isPending || open.isPending;
  let status: string | null = null;
  if (busy) status = entry.local ? t(save.isPending ? "reading.attachments.saving" : "reading.attachments.opening") : t("reading.attachments.downloading");

  const startOpen = () => {
    onFeedback(null);
    if (entry.executable) {
      onNeedsConfirmation(entry);
      return;
    }
    open.mutate(false, {
      onSuccess: (result) => {
        if (result.kind === "needsConfirmation") onNeedsConfirmation(entry);
      },
      onError: (error) => {
        onFeedback({ kind: "error", context: t("reading.attachments.openFailed"), error });
      },
    });
  };
  const startSave = () => {
    onFeedback(null);
    save.mutate(undefined, {
      onSuccess: (result) => {
        if (result.kind === "saved") onFeedback({ kind: "saved", name: entry.name, path: result.path });
      },
      onError: (error) => {
        onFeedback({ kind: "error", context: t("reading.attachments.saveFailed"), error });
      },
    });
  };

  return (
    <div className="flex h-full items-center gap-3 border-b border-border-subtle px-4" data-testid="attachment-entry" data-message={entry.messageId} data-index={entry.index} data-kind={entry.kind}>
      <Icon aria-hidden className={cx("size-(--ui-icon) shrink-0", entry.executable ? "text-warning" : "text-secondary")} strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span title={entry.name} className="truncate text-14 font-medium text-primary select-text" data-testid="attachment-entry-name">
          {entry.name}
        </span>
        <span className="truncate text-12 text-secondary" role={status ? "status" : undefined}>
          {status ??
            t("attachments.view.details", {
              type: fileType(entry.name, entry.mimeType),
              size: entry.local ? size : t("reading.attachments.estimated", { size }),
              sender,
              date: formatFullDate(entry.date, i18n.language),
            })}
        </span>
        <span className="truncate text-12 text-tertiary">{entry.subject || t("attachments.view.noSubject")}</span>
      </div>
      <div className="flex shrink-0">
        <IconButton icon={Mail} label={t("attachments.view.goToMessage", { name: entry.name })} onClick={() => { goToMessage(entry); }} />
        {entry.preview !== null && (
          <IconButton icon={Eye} label={t("attachments.preview.label", { name: entry.name })} disabled={busy} onClick={onPreview} />
        )}
        <IconButton icon={ExternalLink} label={t("attachments.view.open", { name: entry.name })} disabled={busy} onClick={startOpen} />
        <IconButton
          icon={save.isPending ? LoaderCircle : Download}
          label={t("attachments.view.save", { name: entry.name })}
          disabled={busy}
          onClick={startSave}
        />
      </div>
    </div>
  );
}

/** Opens an executable attachment only after the warning (like the reading pane). */
function ConfirmOpen({ entry, onDone, onFeedback }: { entry: AttachmentEntry; onDone: () => void; onFeedback: (f: Feedback | null) => void }) {
  const { t } = useTranslation();
  const open = useOpenAttachment({ messageId: entry.messageId }, entry.index);
  return (
    <ConfirmDialog
      title={t("reading.attachments.warning.title", { name: entry.name })}
      body={t("reading.attachments.warning.body")}
      confirmLabel={t("reading.attachments.warning.confirm")}
      cancelLabel={t("reading.attachments.warning.cancel")}
      onConfirm={() => {
        open.mutate(true, {
          onError: (error) => {
            onFeedback({ kind: "error", context: t("reading.attachments.openFailed"), error });
          },
        });
        onDone();
      }}
      onCancel={onDone}
      data-testid="attachments-view-confirm-open"
    />
  );
}

/**
 * Stage 20 (decision 0060): every attachment of the mail stored on this computer, across the
 * accounts: name, type, size, sender and date; filters by type, sender and account, a search in
 * the file names. A virtual list, keyset pages from the core's index. Open, save and preview go
 * through the reading pane's commands; what is not downloaded yet is fetched then.
 */
export function AttachmentsView() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const nameId = useId();
  const senderId = useId();
  const accountId = useId();
  const [kind, setKind] = useState<AttachmentKind | null>(null);
  const [name, setName] = useState("");
  const [sender, setSender] = useState("");
  const [account, setAccount] = useState<string>("");
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [previewing, setPreviewing] = useState<AttachmentEntry | null>(null);
  const [confirming, setConfirming] = useState<AttachmentEntry | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // The text filters ask the core after a short pause in typing.
  const [debounced, setDebounced] = useState({ name: "", sender: "" });
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced({ name, sender });
    }, 200);
    return () => {
      window.clearTimeout(timer);
    };
  }, [name, sender]);

  const scope: MarkScope = account === "" ? { kind: "all" } : { kind: "account", accountId: account };
  const filter: AttachmentFilter = {
    scope,
    kind,
    sender: debounced.sender.trim() === "" ? null : debounced.sender,
    query: debounced.name.trim() === "" ? null : debounced.name,
  };
  const list = useInfiniteQuery({
    queryKey: [ATTACHMENTS_VIEW_KEY, filter],
    queryFn: ({ pageParam }: { pageParam: AttachmentCursor | null }) => attachmentsViewIpc.list(filter, pageParam, ATTACHMENTS_PAGE_SIZE),
    initialPageParam: null as AttachmentCursor | null,
    getNextPageParam: (last: AttachmentPage) => last.nextCursor,
  });
  const entries = useMemo(() => list.data?.pages.flatMap((page) => page.items) ?? [], [list.data]);
  const count = entries.length + (list.hasNextPage ? 1 : 0);
  // TanStack Virtual keeps its state outside React; the list re-renders from its range (as the
  // message list does).
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 72,
    overscan: 8,
    directDomUpdates: true,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const lastIndex = virtualItems[virtualItems.length - 1]?.index ?? -1;
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = list;
  useEffect(() => {
    if (lastIndex >= entries.length - 1 && hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [lastIndex, entries.length, hasNextPage, isFetchingNextPage, fetchNextPage]);

  const filtered = kind !== null || debounced.name.trim() !== "" || debounced.sender.trim() !== "" || account !== "";

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="attachments-view">
      <div className="flex shrink-0 flex-col gap-3 border-b border-border-subtle bg-surface-list px-4 py-3">
        <div role="group" aria-label={t("attachments.view.kinds")} className="flex flex-wrap items-center gap-1" data-testid="attachments-view-kinds">
          {[null, ...KINDS].map((value) => (
            <button
              key={value ?? "all"}
              type="button"
              aria-pressed={kind === value}
              onClick={() => {
                setKind(value);
              }}
              data-testid={`attachments-kind-${value ?? "all"}`}
              className={cx(
                "rounded-full px-3 py-1 text-13 transition-colors focus-visible:outline-2 focus-visible:outline-focus-ring",
                kind === value ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-hover",
              )}
            >
              {value === null ? t("attachments.view.kind.all") : t(`attachments.view.kind.${value}`)}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <div className="flex min-w-48 flex-1 flex-col gap-1">
            <label htmlFor={nameId} className="text-12 font-medium text-secondary">
              {t("attachments.view.name")}
            </label>
            <div className="flex items-center gap-2 rounded-control border border-border-strong bg-surface-raised px-2">
              <Search aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
              <input
                id={nameId}
                type="search"
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                }}
                placeholder={t("attachments.view.namePlaceholder")}
                className="h-8 min-w-0 flex-1 bg-transparent text-13 text-primary outline-none"
                data-testid="attachments-view-name"
              />
            </div>
          </div>
          <div className="flex min-w-40 flex-col gap-1">
            <label htmlFor={senderId} className="text-12 font-medium text-secondary">
              {t("attachments.view.sender")}
            </label>
            <input
              id={senderId}
              type="search"
              value={sender}
              onChange={(event) => {
                setSender(event.target.value);
              }}
              placeholder={t("attachments.view.senderPlaceholder")}
              className="h-8 rounded-control border border-border-strong bg-surface-raised px-2 text-13 text-primary"
              data-testid="attachments-view-sender"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label htmlFor={accountId} className="text-12 font-medium text-secondary">
              {t("attachments.view.account")}
            </label>
            <select
              id={accountId}
              value={account}
              onChange={(event) => {
                setAccount(event.target.value);
              }}
              className="h-8 rounded-control border border-border-strong bg-surface-raised px-2 text-13 text-primary"
              data-testid="attachments-view-account"
            >
              <option value="">{t("attachments.view.allAccounts")}</option>
              {(accounts.data ?? []).map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.displayName ? t("nav.account", { name: entry.displayName, email: entry.email }) : entry.email}
                </option>
              ))}
            </select>
          </div>
        </div>
        <p className="text-12 text-tertiary">{t("attachments.view.scopeHint")}</p>
      </div>
      {list.isError && (
        <div className="px-4 py-3">
          <ErrorNotice
            context={t("attachments.view.loadFailed")}
            error={list.error}
            onRetry={() => {
              void list.refetch();
            }}
            data-testid="attachments-view-error"
          />
        </div>
      )}
      {list.isSuccess && entries.length === 0 && (
        <div className="flex min-h-0 flex-1 flex-col" data-testid="attachments-view-empty">
          <EmptyState
            icon={Paperclip}
            title={filtered ? t("attachments.view.noMatchTitle") : t("attachments.view.emptyTitle")}
            body={filtered ? t("attachments.view.noMatchBody") : t("attachments.view.emptyBody")}
          />
        </div>
      )}
      {entries.length > 0 && (
        <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto bg-surface" data-testid="attachments-view-list">
          <div ref={virtualizer.containerRef} role="list" aria-label={t("attachments.view.listLabel")} className="relative w-full">
            {virtualItems.map((virtualItem) => {
              const entry = entries[virtualItem.index];
              return (
                <div
                  key={virtualItem.key}
                  ref={virtualizer.measureElement}
                  data-index={virtualItem.index}
                  role="listitem"
                  className="absolute top-0 left-0 h-18 w-full"
                >
                  {entry ? (
                    <AttachmentRow
                      entry={entry}
                      onFeedback={setFeedback}
                      onNeedsConfirmation={setConfirming}
                      onPreview={() => {
                        setFeedback(null);
                        setPreviewing(entry);
                      }}
                    />
                  ) : (
                    <div className="flex h-full items-center gap-3 px-4 text-13 text-tertiary">
                      <LoaderCircle aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
                      {t("list.loadingMore")}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
      {feedback?.kind === "saved" && (
        <p role="status" className="shrink-0 px-4 py-2 text-12 break-all text-success" data-testid="attachments-view-saved">
          {t("attachments.pinned.saved", { name: feedback.name, path: feedback.path })}
        </p>
      )}
      {feedback?.kind === "error" && (
        <div className="shrink-0 px-4 py-2">
          <ErrorNotice context={feedback.context} error={feedback.error} />
        </div>
      )}
      {previewing?.preview && (
        <AttachmentPreviewDialog
          owner={{ messageId: previewing.messageId }}
          attachment={{ index: previewing.index, name: previewing.name, preview: previewing.preview }}
          onClose={() => {
            setPreviewing(null);
          }}
          onSave={() => {
            setPreviewing(null);
          }}
        />
      )}
      {confirming && (
        <ConfirmOpen
          key={rowKey(confirming)}
          entry={confirming}
          onFeedback={setFeedback}
          onDone={() => {
            setConfirming(null);
          }}
        />
      )}
    </div>
  );
}
