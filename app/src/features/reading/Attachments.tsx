import { Download, Info, LoaderCircle, SquareArrowOutUpRight, TriangleExclamationPoint } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import type { AttachmentInfo, EmlFileId, IpcError, MessageId } from "../../services/ipc";
import { ATTACHMENT_ICONS, attachmentKind, fileType, formatSize } from "./format";
import { useOpenAttachment, useSaveAttachment, type AttachmentOwner } from "./queries";

/** What the last action on an attachment left to say, shown under the list in full width. */
type Feedback =
  | { kind: "saved"; name: string; path: string }
  | { kind: "error"; context: string; error: IpcError };

interface AttachmentItemProps {
  owner: AttachmentOwner;
  attachment: AttachmentInfo;
  /** Opening it needs the warning first (the list marked it, or the core asked). */
  onNeedsConfirmation: (attachment: AttachmentInfo) => void;
  onFeedback: (feedback: Feedback | null) => void;
  confirming: boolean;
}

/** One attachment in one line: type symbol, name, type and size, Save and Open. */
function AttachmentItem({ owner, attachment, onNeedsConfirmation, onFeedback, confirming }: AttachmentItemProps) {
  const { t, i18n } = useTranslation();
  const save = useSaveAttachment(owner, attachment.index);
  const open = useOpenAttachment(owner, attachment.index);
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

  let status: string | null = null;
  if (save.isPending) status = working("saving");
  else if (open.isPending) status = working("opening");

  return (
    <li
      className={cx(
        "flex w-(--ui-attachment-width) max-w-full items-center gap-3 self-stretch rounded-card border bg-canvas py-2 ps-3 pe-1",
        attachment.executable || confirming ? "border-warning" : "border-border",
      )}
      data-testid="attachment"
      data-index={attachment.index}
    >
      <Icon
        aria-hidden
        className={cx("size-(--ui-icon) shrink-0", attachment.executable ? "text-warning" : "text-secondary")}
        strokeWidth={1.75}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <p title={attachment.name} className="truncate text-13 font-medium text-primary select-text" data-testid="attachment-name">
          {attachment.name}
        </p>
        <p className="truncate text-12 text-secondary" data-testid="attachment-details" role={status ? "status" : undefined}>
          {status ?? details}
        </p>
        {/* Never cut off: the warning must stay readable in a narrow card. */}
        {!status && attachment.executable && (
          <p className="text-12 font-medium text-warning">{t("reading.attachments.executable")}</p>
        )}
        {!status && !attachment.local && <p className="text-12 text-secondary">{t("reading.attachments.notLocal")}</p>}
      </div>
      <div className="flex shrink-0">
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
          />
        ))}
      </ul>
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
      {feedback?.kind === "error" && <ErrorNotice context={feedback.context} error={feedback.error} />}
    </section>
  );
}
