import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../../components/ConfirmDialog";

interface AttachmentReminderProps {
  /** The word of the user's text that speaks of an attachment, as the core found it. */
  word: string;
  onSendAnyway: () => void;
  /** "Add attachment": the file dialog (composer) or the full composer (quick reply). */
  onAddAttachment: () => void;
  onCancel: () => void;
}

/**
 * Stage 20 (decision 0058): the question before sending when the text speaks of an attachment
 * but none is there. Never asked for sends of the outbox; can be turned off under Settings →
 * Writing.
 */
export function AttachmentReminder({ word, onSendAnyway, onAddAttachment, onCancel }: AttachmentReminderProps) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      tone="neutral"
      title={t("composer.attachmentReminder.title")}
      body={t("composer.attachmentReminder.body", { word })}
      confirmLabel={t("composer.attachmentReminder.sendAnyway")}
      secondary={{ label: t("composer.attachmentReminder.add"), onClick: onAddAttachment }}
      cancelLabel={t("composer.attachmentReminder.cancel")}
      onConfirm={onSendAnyway}
      onCancel={onCancel}
      data-testid="attachment-reminder"
    />
  );
}
