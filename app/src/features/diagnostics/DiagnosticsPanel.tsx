import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { toCoreError } from "../../services/ipc";
import { useExportDiagnostics } from "./queries";
import { cx } from "../../components/cx";
import { SETTING_ANCHOR_CLASS, SETTING_BLOCK, settingAnchor } from "../../components/settingAnchor";

/**
 * Stage 15, part of Settings → About: "Export diagnostics…" with what the file holds and what it
 * never holds. The core shows the save dialog, then collects, redacts and writes a plain-text
 * file; closing the dialog changes nothing. Nothing is ever sent anywhere.
 */
export function DiagnosticsPanel() {
  const { t } = useTranslation();
  const headingId = useId();
  const containsId = useId();
  const neverId = useId();
  const exportDiagnostics = useExportDiagnostics();
  const result = exportDiagnostics.isSuccess ? exportDiagnostics.data : null;
  const savedPath = result?.kind === "saved" ? result.path : null;
  const run = () => {
    exportDiagnostics.mutate();
  };

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3" data-testid="diagnostics-panel" {...SETTING_BLOCK}>
      {/* Stage 22 (decision 0073): found by the settings search with its first sentence. */}
      <div {...settingAnchor(t("settings.about.diagnostics.title"))} className={cx("flex flex-col gap-3", SETTING_ANCHOR_CLASS)}>
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {t("settings.about.diagnostics.title")}
        </h3>
        <p className="text-14 text-secondary">{t("settings.about.diagnostics.intro")}</p>
      </div>
      <p id={containsId} className="text-14 text-secondary">
        {t("settings.about.diagnostics.contains")}
      </p>
      <p id={neverId} className="text-14 text-secondary">
        {t("settings.about.diagnostics.never")}
      </p>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="secondary"
          disabled={exportDiagnostics.isPending}
          aria-describedby={`${containsId} ${neverId}`}
          onClick={run}
          data-testid="diagnostics-export"
        >
          {t("settings.about.diagnostics.export")}
        </Button>
        {exportDiagnostics.isPending && (
          <span role="status" className="text-13 text-secondary">
            {t("settings.about.diagnostics.exporting")}
          </span>
        )}
      </div>
      {savedPath !== null && (
        <p role="status" className="text-13 break-all text-primary select-text" data-testid="diagnostics-saved">
          {t("settings.about.diagnostics.saved", { path: savedPath })}
        </p>
      )}
      {exportDiagnostics.isError && (
        <ErrorNotice
          context={t("settings.about.diagnostics.failed")}
          error={toCoreError(exportDiagnostics.error)}
          onRetry={run}
          data-testid="diagnostics-error"
        />
      )}
    </section>
  );
}
