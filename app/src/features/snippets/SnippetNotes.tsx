import { useTranslation } from "react-i18next";
import { Notice } from "../../components/Notice";
import type { SanitizeReport } from "../../services/ipc";

/** What sanitising changed when a signature or template was saved; nothing when it changed nothing. */
export function SnippetNotes({ notes, "data-testid": testId }: { notes: SanitizeReport | null; "data-testid"?: string }) {
  const { t } = useTranslation();
  if (!notes || (notes.imagesRemoved === 0 && !notes.formattingSimplified)) return null;
  return (
    <Notice tone="warning" role="status" title={t("snippets.notes.title")} {...(testId && { "data-testid": testId })}>
      {notes.imagesRemoved > 0 && <p>{t("snippets.notes.imagesRemoved", { count: notes.imagesRemoved })}</p>}
      {notes.formattingSimplified && <p>{t("snippets.notes.formattingSimplified")}</p>}
    </Notice>
  );
}

/** Whether saving changed something the user should know about. */
export function hasNotes(notes: SanitizeReport): boolean {
  return notes.imagesRemoved > 0 || notes.formattingSimplified;
}
