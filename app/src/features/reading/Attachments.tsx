import { Download, Eye, FolderDown, Info, LoaderCircle, Pin, PinOff, SquareArrowOutUpRight, TriangleExclamationPoint } from "lucide-react";
import { useEffect, useId, useImperativeHandle, useRef, useState, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import type { AttachmentInfo, EmlFileId, IpcError, MessageId } from "../../services/ipc";
import { useAttachmentPins, useSaveAllAttachments, useSetAttachmentPinned } from "./attachmentQueries";
import { AttachmentPreviewDialog } from "./AttachmentPreview";
import { ATTACHMENT_ICONS, attachmentKind, fileType, formatSize } from "./format";
import { useOpenAttachment, useSaveAttachment, type AttachmentOwner } from "./queries";
import { useSaveAllRequest } from "./saveAllRequest";

/** What the last action on an attachment left to say, shown under the list in full width. */
type Feedback =
  | { kind: "saved"; name: string; path: string }
  /** Stage 16 (decision 0036): "Save all" into one folder. */
  | { kind: "savedAll"; folder: string; saved: number; renamed: number }
  | { kind: "error"; context: string; error: IpcError };

/** What the preview dialog's Save and Open reach of a card. */
interface ItemHandle {
  save: () => void;
  open: () => void;
}

interface AttachmentItemProps {
  owner: AttachmentOwner;
  attachment: AttachmentInfo;
  /** Opening it needs the warning first (the list marked it, or the core asked). */
  onNeedsConfirmation: (attachment: AttachmentInfo) => void;
  onFeedback: (feedback: Feedback | null) => void;
  confirming: boolean;
  /** Stage 16: pinned or not; null where attachments cannot be pinned (an opened `.eml` file). */
  pinned: boolean | null;
  onTogglePin: () => void;
  pinBusy: boolean;
  /** Stage 16: opens the preview (only offered when the core has one for this attachment). */
  onPreview: () => void;
  handle: Ref<ItemHandle>;
}

/** One attachment in one line: type symbol, name, type and size, Preview, Pin, Save and Open. */
function AttachmentItem({
  owner,
  attachment,
  onNeedsConfirmation,
  onFeedback,
  confirming,
  pinned,
  onTogglePin,
  pinBusy,
  onPreview,
  handle,
}: AttachmentItemProps) {
  const { t, i18n } = useTranslation();
  const save = useSaveAttachment(owner, attachment.index);
  const open = useOpenAttachment(owner, attachment.index);
  const hintId = useId();
  const Icon = ATTACHMENT_ICONS[attachmentKind(attachment.mimeType, attachment.executable)];
  const size = formatSize(attachment.size, i18n.language);
  const details = t("reading.attachments.details", {
    type: fileType(attachment.name, attachment.mimeType),
    size: attachment.local ? size : t("reading.attachments.estimated", { size }),
  });
  // A large message keeps its attachments on the server until they are needed.
  const working = (action: "saving" | "opening") =>
    attachment.local ? t(`reading.attachments.${action}`) : t("reading.attachments.downloading");
  const busy = save.isPending || open.isPending;

  const startSave = () => {
    onFeedback(null);
    save.mutate(undefined, {
      onSuccess: (result) => {
        if (result.kind === "saved") onFeedback({ kind: "saved", name: attachment.name, path: result.path });
      },
      onError: (error) => {
        onFeedback({ kind: "error", context: t("reading.attachments.saveFailed"), error });
      },
    });
  };
  const startOpen = () => {
    onFeedback(null);
    if (attachment.executable) {
      onNeedsConfirmation(attachment);
      return;
    }
    open.mutate(false, {
      // The core decides too: ask, if it says so for a file the list did not mark.
      onSuccess: (result) => {
        if (result.kind === "needsConfirmation") onNeedsConfirmation(attachment);
      },
      onError: (error) => {
        onFeedback({ kind: "error", context: t("reading.attachments.openFailed"), error });
      },
    });
  };

  useImperativeHandle(handle, () => ({ save: startSave, open: startOpen }));

  let status: string | null = null;
  if (save.isPending) status = working("saving");
  else if (open.isPending) status = working("opening");
  const previewable = attachment.preview !== null;

  const text = (
    <>
      <Icon
        aria-hidden
        className={cx("size-(--ui-icon) shrink-0", attachment.executable ? "text-warning" : "text-secondary")}
        strokeWidth={1.75}
      />
      <span className="flex min-w-0 flex-1 flex-col text-start">
        <span title={attachment.name} className="truncate text-13 font-medium text-primary select-text" data-testid="attachment-name">
          {attachment.name}
        </span>
        <span className="truncate text-12 text-secondary" data-testid="attachment-details" role={status ? "status" : undefined}>
          {status ?? details}
        </span>
        {/* Never cut off: the warning must stay readable in a narrow card. */}
        {!status && attachment.executable && (
          <span className="text-12 font-medium text-warning">{t("reading.attachments.executable")}</span>
        )}
        {!status && !attachment.local && <span className="text-12 text-secondary">{t("reading.attachments.notLocal")}</span>}
        {pinned === true && (
          <span className="flex items-center gap-1 text-12 text-accent" data-testid="attachment-pinned">
            <Pin aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("attachments.pin.pinned")}
          </span>
        )}
      </span>
    </>
  );

  return (
    <li
      className={cx(
        "flex w-(--ui-attachment-width) max-w-full items-center gap-2 self-stretch rounded-card border bg-canvas py-2 ps-3 pe-1",
        attachment.executable || confirming ? "border-warning" : "border-border",
      )}
      data-testid="attachment"
      data-index={attachment.index}
    >
      {/* Stage 16: with a preview the card's text is a button: a click, Enter or Space previews. */}
      {previewable ? (
        <button
          type="button"
          onClick={onPreview}
          aria-describedby={hintId}
          className="flex min-w-0 flex-1 items-center gap-2 rounded-control text-start"
          data-testid="attachment-card"
        >
          {text}
          <span id={hintId} className="sr-only">
            {t("attachments.preview.cardHint")}
          </span>
        </button>
      ) : (
        <div className="flex min-w-0 flex-1 items-center gap-2">{text}</div>
      )}
      <div className="flex shrink-0">
        {previewable && (
          <IconButton icon={Eye} label={t("attachments.preview.label", { name: attachment.name })} onClick={onPreview} />
        )}
        {pinned !== null && (
          <IconButton
            icon={pinned ? PinOff : Pin}
            label={t(pinned ? "attachments.pin.unpin" : "attachments.pin.pin", { name: attachment.name })}
            disabled={pinBusy}
            onClick={onTogglePin}
          />
        )}
        <IconButton
          icon={save.isPending ? LoaderCircle : Download}
          label={t("reading.attachments.saveLabel", { name: attachment.name })}
          disabled={busy}
          onClick={startSave}
        />
        <IconButton
          icon={open.isPending ? LoaderCircle : SquareArrowOutUpRight}
          label={t("reading.attachments.openLabel", { name: attachment.name })}
          disabled={busy || confirming}
          onClick={startOpen}
        />
      </div>
    </li>
  );
}

interface ExecutableWarningProps {
  owner: AttachmentOwner;
  attachment: AttachmentInfo;
  onDone: () => void;
  onFeedback: (feedback: Feedback | null) => void;
}

/**
 * The warning before a file that can run code is opened (programs, scripts, documents with
 * macros). Opening needs this second, explicit click; the core gets `confirmedExecutable`.
 */
function ExecutableWarning({ owner, attachment, onDone, onFeedback }: ExecutableWarningProps) {
  const { t } = useTranslation();
  const open = useOpenAttachment(owner, attachment.index);
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    ref.current?.focus();
  }, []);

  return (
    <div
      ref={ref}
      tabIndex={-1}
      role="group"
      aria-labelledby={titleId}
      className="flex max-w-(--ui-reading-max-width) gap-3 rounded-card border border-warning bg-canvas p-4"
      data-testid="attachment-warning"
    >
      <TriangleExclamationPoint aria-hidden className="size-(--ui-icon) shrink-0 text-warning" strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <p id={titleId} className="font-semibold break-words text-primary">
          {t("reading.attachments.warning.title", { name: attachment.name })}
        </p>
        <p className="text-13 text-secondary">{t("reading.attachments.warning.body")}</p>
        <div className="flex flex-wrap gap-2 pt-1">
          <Button
            variant="danger"
            disabled={open.isPending}
            onClick={() => {
              open.mutate(true, {
                onSuccess: onDone,
                onError: (error) => {
                  onFeedback({ kind: "error", context: t("reading.attachments.openFailed"), error });
                },
              });
            }}
            data-testid="attachment-confirm"
          >
            {open.isPending
              ? attachment.local
                ? t("reading.attachments.opening")
                : t("reading.attachments.downloading")
              : t("reading.attachments.warning.confirm")}
          </Button>
          <Button variant="secondary" disabled={open.isPending} onClick={onDone} data-testid="attachment-cancel">
            {t("reading.attachments.warning.cancel")}
          </Button>
        </div>
      </div>
    </div>
  );
}

type AttachmentsProps = (
  | { messageId: MessageId; fileId?: undefined }
  /** Stage 9 (decision 0027): the attachments of an opened `.eml` file, saved and opened by the core too. */
  | { fileId: EmlFileId; messageId?: undefined }
) & {
  attachments: readonly AttachmentInfo[];
  /** Only the text parts of this message are stored; the attachments come on demand. */
  partial: boolean;
};

/** The attachments under the head: one line each; warning, result and errors below the list. */
export function Attachments({ messageId, fileId, attachments, partial }: AttachmentsProps) {
  const { t } = useTranslation();
  const owner: AttachmentOwner = fileId !== undefined ? { fileId } : { messageId };
  const [confirm, setConfirm] = useState<AttachmentInfo | null>(null);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  // Stage 16 (decision 0036): preview, "Save all", pins (stored messages only).
  const [previewing, setPreviewing] = useState<AttachmentInfo | null>(null);
  const items = useRef(new Map<number, ItemHandle>());
  const saveAll = useSaveAllAttachments(owner);
  const pinMessage = messageId ?? null;
  const pins = useAttachmentPins(attachments.length > 0 ? pinMessage : null);
  const setPinned = useSetAttachmentPinned();
  const pinnedIndexes = new Set(pins.data ?? []);

  const startSaveAll = () => {
    setFeedback(null);
    saveAll.mutate(undefined, {
      onSuccess: (result) => {
        if (result.kind === "saved") setFeedback({ kind: "savedAll", folder: result.folder, saved: result.saved, renamed: result.renamed });
      },
      onError: (error) => {
        setFeedback({ kind: "error", context: t("attachments.saveAll.failed"), error });
      },
    });
  };
  // The palette's "Save all attachments" for this message runs here, with the same status line.
  useSaveAllRequest(pinMessage, attachments.length, startSaveAll);

  const togglePin = (attachment: AttachmentInfo) => {
    if (pinMessage === null) return;
    setFeedback(null);
    setPinned.mutate(
      { messageId: pinMessage, index: attachment.index, pinned: !pinnedIndexes.has(attachment.index) },
      {
        onError: (error) => {
          setFeedback({ kind: "error", context: t("attachments.pin.failed"), error });
        },
      },
    );
  };

  const partialNote = partial && (
    <p className="flex items-center gap-2 text-12 text-secondary" data-testid="reading-partial">
      <Info aria-hidden className="size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
      {t("reading.partial")}
    </p>
  );
  if (attachments.length === 0) return partialNote || null;

  return (
    <section
      aria-label={t("reading.attachments.label", { count: attachments.length })}
      className="flex flex-col gap-2"
      data-testid="attachments"
    >
      {partialNote}
      <ul className="flex flex-wrap gap-2">
        {attachments.map((attachment) => (
          <AttachmentItem
            key={attachment.index}
            owner={owner}
            attachment={attachment}
            confirming={confirm?.index === attachment.index}
            onNeedsConfirmation={setConfirm}
            onFeedback={setFeedback}
            pinned={pinMessage === null || !pins.data ? null : pinnedIndexes.has(attachment.index)}
            pinBusy={setPinned.isPending}
            onTogglePin={() => {
              togglePin(attachment);
            }}
            onPreview={() => {
              setFeedback(null);
              setPreviewing(attachment);
            }}
            handle={(handle: ItemHandle | null) => {
              if (handle) items.current.set(attachment.index, handle);
              else items.current.delete(attachment.index);
            }}
          />
        ))}
      </ul>
      {attachments.length >= 2 && (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="secondary"
            disabled={saveAll.isPending}
            onClick={startSaveAll}
            aria-label={t("attachments.saveAll.label", { count: attachments.length })}
            data-testid="attachments-save-all"
          >
            {saveAll.isPending ? (
              <LoaderCircle aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            ) : (
              <FolderDown aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            )}
            {saveAll.isPending ? t("attachments.saveAll.saving") : t("attachments.saveAll.action")}
          </Button>
        </div>
      )}
      {previewing?.preview && (
        <AttachmentPreviewDialog
          owner={owner}
          attachment={{ index: previewing.index, name: previewing.name, preview: previewing.preview }}
          onClose={() => {
            setPreviewing(null);
          }}
          onSave={() => {
            items.current.get(previewing.index)?.save();
          }}
          onOpen={() => {
            items.current.get(previewing.index)?.open();
          }}
        />
      )}
      {confirm && (
        <ExecutableWarning
          key={confirm.index}
          owner={owner}
          attachment={confirm}
          onFeedback={setFeedback}
          onDone={() => {
            setConfirm(null);
          }}
        />
      )}
      {feedback?.kind === "saved" && (
        <p role="status" className="text-12 break-all text-success">
          {t("reading.attachments.saved", { name: feedback.name, path: feedback.path })}
        </p>
      )}
      {feedback?.kind === "savedAll" && (
        <p role="status" className="text-12 break-all text-success" data-testid="attachments-saved-all">
          {t("attachments.saveAll.saved", { count: feedback.saved, folder: feedback.folder })}
          {feedback.renamed > 0 && ` ${t("attachments.saveAll.renamed", { count: feedback.renamed })}`}
        </p>
      )}
      {feedback?.kind === "error" && <ErrorNotice context={feedback.context} error={feedback.error} />}
    </section>
  );
}
