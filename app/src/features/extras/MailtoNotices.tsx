import { useTranslation } from "react-i18next";
import { Notice } from "../../components/Notice";
import type { MailtoField, MailtoNotice } from "../../services/ipc";
import type { Translate } from "./calendar";

/** One hint about a `mailto:` link in words. */
export function mailtoNoticeText(notice: MailtoNotice, t: Translate): string {
  const field = (name: MailtoField) => t(`extras.mailto.fields.${name}`);
  switch (notice.kind) {
    case "bcc_present":
      return t("extras.mailto.notices.bcc_present");
    case "header_ignored":
      return t("extras.mailto.notices.header_ignored", { name: notice.name });
    case "address_invalid":
    case "message_id_invalid":
      return t(`extras.mailto.notices.${notice.kind}`, { value: notice.value });
    case "duplicate_ignored":
    case "truncated":
    case "invalid_encoding":
    case "control_characters_removed":
      return t(`extras.mailto.notices.${notice.kind}`, { field: field(notice.field) });
    case "notices_omitted":
      return t("extras.mailto.notices.notices_omitted", { count: notice.count });
  }
}

interface MailtoNoticesProps {
  notices: readonly MailtoNotice[];
  /** The link named a message it answers, which a new draft cannot carry. */
  inReplyTo: boolean;
}

/**
 * What the user should know about the `mailto:` link this draft came from, before sending: blind
 * copies first (the core puts that hint first), then ignored or shortened parts.
 */
export function MailtoNotices({ notices, inReplyTo }: MailtoNoticesProps) {
  const { t } = useTranslation();
  const text = t as unknown as Translate;
  const lines = notices.map((notice) => mailtoNoticeText(notice, text));
  if (inReplyTo) lines.push(t("extras.mailto.notices.inReplyTo"));
  if (lines.length === 0) return null;
  const bcc = notices.some((notice) => notice.kind === "bcc_present");
  return (
    <div className="px-4 pt-4" data-testid="mailto-notices">
      <Notice tone={bcc ? "warning" : "info"} title={t("extras.mailto.noticesTitle")}>
        <ul className="flex list-disc flex-col gap-1 ps-4">
          {lines.map((line, index) => (
            <li key={index} data-testid="mailto-notice">
              {line}
            </li>
          ))}
        </ul>
      </Notice>
    </div>
  );
}
