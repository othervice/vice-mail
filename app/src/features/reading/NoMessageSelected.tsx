import { Mail } from "lucide-react";
import { useTranslation } from "react-i18next";

/**
 * Stage 18 (task F): the reading pane's place while no message is selected (an empty folder, a
 * cleared selection): a quiet note, so the list keeps its reading size instead of spanning the
 * window with long rows. Nothing to focus or click.
 */
export function NoMessageSelected() {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-2 bg-surface px-8 text-center" data-testid="reading-empty">
      <Mail aria-hidden className="size-(--ui-icon) text-tertiary" strokeWidth={1.5} />
      <p className="text-14 font-medium text-secondary">{t("reading.noneSelected.title")}</p>
      <p className="max-w-(--ui-form-max-width) text-13 text-tertiary">{t("reading.noneSelected.body")}</p>
    </div>
  );
}
