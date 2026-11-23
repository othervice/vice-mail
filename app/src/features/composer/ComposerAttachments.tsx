import { File, Paperclip, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { Notice } from "../../components/Notice";
import { LARGE_ATTACHMENT_WARNING_BYTES, type DraftAttachment } from "../../services/ipc";
import { formatSize } from "./addresses";

interface ComposerAttachmentsProps {
  attachments: readonly DraftAttachment[];
  busy: boolean;
  disabled: boolean;
  onPick: () => void;
  onRemove: (attachment: DraftAttachment) => void;
}

export function totalSize(attachments: readonly DraftAttachment[]): number {
  return attachments.reduce((sum, a) => sum + a.size, 0);
}

/**
 * The attachment bar at the bottom of the composer (docs/DESIGN.md). Files are picked in the
 * system dialog of the core or dropped on the window; the page never reads a file itself.
 */
export function ComposerAttachments({ attachments, busy, disabled, onPick, onRemove }: ComposerAttachmentsProps) {
  const { t, i18n } = useTranslation();
  const total = totalSize(attachments);
  return (
    <div className="flex shrink-0 flex-col gap-2 border-t border-border px-4 py-2" data-testid="composer-attachments">
      {attachments.length > 0 && (
        <ul aria-label={t("composer.attachments.list", { count: attachments.length })} className="flex flex-wrap gap-2">
          {attachments.map((attachment) => (
            <li
              key={attachment.id}
              className="flex max-w-(--ui-attachment-width) items-center gap-2 rounded-control border border-border bg-canvas py-1 ps-2 pe-1 text-13"
              data-testid="composer-attachment"
            >
              <File aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
              <span className="min-w-0 truncate text-primary" title={attachment.fileName}>
                {attachment.fileName}
              </span>
              <span className="shrink-0 text-12 text-secondary tabular-nums">{formatSize(attachment.size, i18n.language)}</span>
              <button
                type="button"
                disabled={disabled}
                aria-label={t("composer.attachments.remove", { name: attachment.fileName })}
                onClick={() => {
                  onRemove(attachment);
                }}
                className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
                data-testid="composer-attachment-remove"
              >
                <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {total > LARGE_ATTACHMENT_WARNING_BYTES && (
        <Notice
          tone="warning"
          title={t("composer.attachments.largeTitle", { size: formatSize(total, i18n.language) })}
          data-testid="composer-attachments-large"
        >
          <p>{t("composer.attachments.largeHint")}</p>
        </Notice>
      )}
      <div className="flex items-center gap-3">
        <Button variant="quiet" onClick={onPick} disabled={disabled || busy} data-testid="composer-attach">
          <Paperclip aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {busy ? t("composer.attachments.adding") : t("composer.attachments.add")}
        </Button>
        <p className="text-12 text-tertiary">{t("composer.attachments.dropHint")}</p>
      </div>
    </div>
  );
}
