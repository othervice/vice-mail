import { Download, Eye, LoaderCircle, Mail, Paperclip, PinOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import type { IpcError, MarkScope, PinnedAttachment } from "../../services/ipc";
import { selectWhenListed } from "../../stores/listSelect";
import { useUiStore } from "../../stores/ui";
import { formatFullDate } from "../messages/rows";
import { openMessageInWindow } from "../windows/open";
import { usePinnedAttachments, useSetAttachmentPinned } from "./attachmentQueries";
import { AttachmentPreviewDialog } from "./AttachmentPreview";
import { ATTACHMENT_ICONS, attachmentKind, fileType, formatSize } from "./format";
import { useSaveAttachment } from "./queries";

type Feedback = { kind: "saved"; name: string; path: string } | { kind: "error"; context: string; error: IpcError };

/** Shows the message of a pinned attachment where mail is read: selected in its folder's list. */
function openMessage(entry: PinnedAttachment): void {
  const { accountId, mailboxId, id: messageId } = entry.message;
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

const rowKey = (entry: PinnedAttachment) => `${String(entry.message.id)}/${String(entry.index)}`;

interface RowProps {
  entry: PinnedAttachment;
  onPreview: () => void;
  onFeedback: (feedback: Feedback | null) => void;
  /** The preview's Save reaches the row's save, with the row's status. */
  saveRef: (save: (() => void) | null) => void;
}

function PinnedRow({ entry, onPreview, onFeedback, saveRef }: RowProps) {
  const { t, i18n } = useTranslation();
  const save = useSaveAttachment({ messageId: entry.message.id }, entry.index);
  const unpin = useSetAttachmentPinned();
  // Pinned attachments are never flagged as programs here: opening goes through the message.
  const Icon = ATTACHMENT_ICONS[attachmentKind(entry.mimeType, false)];
  const sender = entry.message.from ? entry.message.from.name || entry.message.from.email : t("attachments.pinned.unknownSender");
  const startSave = () => {
    onFeedback(null);
    save.mutate(undefined, {
      onSuccess: (result) => {
        if (result.kind === "saved") onFeedback({ kind: "saved", name: entry.name, path: result.path });
      },
      onError: (error) => {
        onFeedback({ kind: "error", context: t("attachments.pinned.saveFailed"), error });
      },
    });
  };
  useEffect(() => {
    saveRef(startSave);
    return () => {
      saveRef(null);
    };
  });

  return (
    <li className="flex items-center gap-3 px-4 py-3" data-testid="pinned-attachment" data-message={entry.message.id} data-index={entry.index}>
      <Icon aria-hidden className="size-(--ui-icon) shrink-0 text-secondary" strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span title={entry.name} className="truncate text-14 font-medium text-primary select-text" data-testid="pinned-attachment-name">
          {entry.name}
        </span>
        <span className="truncate text-12 text-secondary" role={save.isPending ? "status" : undefined}>
          {save.isPending
            ? t("reading.attachments.saving")
            : t("reading.attachments.details", { type: fileType(entry.name, entry.mimeType), size: formatSize(entry.size, i18n.language) })}
        </span>
        <span className="truncate text-13 text-primary" data-testid="pinned-attachment-subject">
          {entry.message.subject || t("attachments.pinned.noSubject")}
        </span>
        <span className="truncate text-12 text-secondary">
          {t("attachments.pinned.from", { sender, date: formatFullDate(entry.message.date, i18n.language) })}
        </span>
      </div>
      <div className="flex shrink-0">
        <IconButton
          icon={Mail}
          label={t("attachments.pinned.openMessageLabel", { name: entry.name })}
          onClick={() => {
            openMessage(entry);
          }}
        />
        {entry.preview !== null && (
          <IconButton icon={Eye} label={t("attachments.preview.label", { name: entry.name })} onClick={onPreview} />
        )}
        <IconButton
          icon={save.isPending ? LoaderCircle : Download}
          label={t("attachments.pinned.saveLabel", { name: entry.name })}
          disabled={save.isPending}
          onClick={startSave}
        />
        <IconButton
          icon={PinOff}
          label={t("attachments.pinned.unpinLabel", { name: entry.name })}
          disabled={unpin.isPending}
          onClick={() => {
            onFeedback(null);
            unpin.mutate(
              { messageId: entry.message.id, index: entry.index, pinned: false },
              {
                onError: (error) => {
                  onFeedback({ kind: "error", context: t("attachments.pin.failed"), error });
                },
              },
            );
          }}
        />
      </div>
    </li>
  );
}

/**
 * Stage 16 (decision 0036): "Pinned attachments", next to the reading list. The attachments the
 * user pinned in messages of the scope, the newest pin first, with their message; open the
 * message, preview, save or unpin. Pins are local to the app (synced, never on the server).
 */
export function PinnedAttachmentsView({ scope }: { scope: MarkScope }) {
  const { t } = useTranslation();
  const list = usePinnedAttachments(scope);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [previewing, setPreviewing] = useState<PinnedAttachment | null>(null);
  const saves = useRef(new Map<string, () => void>());

  if (list.isPending) return null;
  if (list.isError) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("attachments.pinned.loadFailed")}
          error={list.error}
          onRetry={() => {
            void list.refetch();
          }}
          data-testid="pinned-attachments-error"
        />
      </div>
    );
  }
  const entries = list.data;
  if (entries.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col" data-testid="pinned-attachments-empty">
        <EmptyState icon={Paperclip} title={t("attachments.pinned.emptyTitle")} body={t("attachments.pinned.emptyBody")} />
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 py-4" data-testid="pinned-attachments">
      <p className="text-12 text-secondary">{t("attachments.pinned.count", { count: entries.length })}</p>
      <ul
        aria-label={t("attachments.pinned.listLabel")}
        className="flex max-w-(--ui-content-max-width) flex-col divide-y divide-border rounded-card border border-border bg-surface"
      >
        {entries.map((entry) => (
          <PinnedRow
            key={rowKey(entry)}
            entry={entry}
            onFeedback={setFeedback}
            onPreview={() => {
              setFeedback(null);
              setPreviewing(entry);
            }}
            saveRef={(save) => {
              if (save) saves.current.set(rowKey(entry), save);
              else saves.current.delete(rowKey(entry));
            }}
          />
        ))}
      </ul>
      {feedback?.kind === "saved" && (
        <p role="status" className="text-12 break-all text-success">
          {t("attachments.pinned.saved", { name: feedback.name, path: feedback.path })}
        </p>
      )}
      {feedback?.kind === "error" && <ErrorNotice context={feedback.context} error={feedback.error} />}
      {previewing?.preview && (
        <AttachmentPreviewDialog
          owner={{ messageId: previewing.message.id }}
          attachment={{ index: previewing.index, name: previewing.name, preview: previewing.preview }}
          onClose={() => {
            setPreviewing(null);
          }}
          onSave={() => {
            saves.current.get(rowKey(previewing))?.();
          }}
        />
      )}
    </div>
  );
}
