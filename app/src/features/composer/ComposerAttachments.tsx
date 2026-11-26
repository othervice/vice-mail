import { File, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Notice } from "../../components/Notice";
import { LARGE_ATTACHMENT_WARNING_BYTES, type DraftAttachment } from "../../services/ipc";
import { formatSize } from "./addresses";

interface ComposerAttachmentsProps {
  attachments: readonly DraftAttachment[];
  /** Files are being added (the system dialog or a drop): said once, under the chips. */
  busy: boolean;
  disabled: boolean;
  onRemove: (attachment: DraftAttachment) => void;
}

export function totalSize(attachments: readonly DraftAttachment[]): number {
  return attachments.reduce((sum, a) => sum + a.size, 0);
}

/**
 * The attachments of a draft (docs/DESIGN.md), stage 23 (decision 0086): compact chips above the
 * composer's send row; adding files is the paperclip in that row (the core's system dialog) or a
 * drop on the window. The page never reads a file by its path.
 */
export function ComposerAttachments({ attachments, busy, disabled, onRemove }: ComposerAttachmentsProps) {
  const { t, i18n } = useTranslation();
  const total = totalSize(attachments);
  if (attachments.length === 0 && !busy) return null;
  return (
    <div className="flex shrink-0 flex-col gap-2" data-testid="composer-attachments">
      {attachments.length > 0 && (
        <ul aria-label={t("composer.attachments.list", { count: attachments.length })} className="flex flex-wrap gap-2">
          {attachments.map((attachment) => (
            <li
              key={attachment.id}
              className="flex h-8 max-w-(--ui-attachment-width) items-center gap-2 rounded-tile bg-badge ps-2 pe-1 text-13"
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
                className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-surface-raised hover:text-primary"
                data-testid="composer-attachment-remove"
              >
                <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {busy && (
        <p role="status" className="text-12 text-secondary" data-testid="composer-attaching">
          {t("composer.attachments.adding")}
        </p>
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
    </div>
  );
}
