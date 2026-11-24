import { useQuery } from "@tanstack/react-query";
import { DatabaseZap } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ipc } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";

export const startupProblemQueryKey = ["startupProblem"] as const;

/** The last part of a path, with either separator (the core runs on macOS and Windows). */
function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * Above every view while the local database did not open at this start (audit 24.11.2026): what
 * happened (the error's text by its code), that the mail itself is on the server, the copies the
 * app took before updating its data and how to go back to one, and where to get a diagnostics
 * file. Nothing at all when the database opened. Asked once: the state cannot change before the
 * next start.
 */
export function StartupNotice() {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const problem = useQuery({ queryKey: startupProblemQueryKey, queryFn: ipc.getStartupProblem, staleTime: Infinity }).data;
  if (!problem) return null;
  const { error } = problem;
  const newest = problem.backups[0];
  const technical = [error.code, error.message, error.detail, t("startupProblem.database", { path: problem.databasePath })]
    .filter(Boolean)
    .join("\n");
  return (
    <div className="px-6 pt-4">
      <section
        role="alert"
        aria-labelledby="startup-problem-title"
        data-testid="startup-problem"
        data-error-code={error.code}
        className="flex gap-3 rounded-card border border-danger bg-surface p-4"
      >
        <DatabaseZap aria-hidden className="size-(--ui-icon) shrink-0 text-danger" strokeWidth={1.75} />
        <div className="flex min-w-0 flex-1 flex-col gap-2 text-13">
          <h2 id="startup-problem-title" className="font-semibold text-primary">
            {t("startupProblem.title")}
          </h2>
          <p className="text-secondary">
            {t(`errors.codes.${error.code}.title`)} {t(`errors.codes.${error.code}.hint`)}
          </p>
          <p className="text-secondary">{t("startupProblem.mailSafe")}</p>
          {error.code === "database_too_new" ? (
            <p className="text-secondary">{t("startupProblem.tooNew")}</p>
          ) : newest ? (
            <>
              <p className="text-secondary select-text" data-testid="startup-problem-backups">
                {t("startupProblem.backups", { count: problem.backups.length, folder: problem.backupDir, newest })}
              </p>
              <p className="text-secondary select-text">
                {t("startupProblem.restore", { database: problem.databasePath, name: fileName(problem.databasePath) })}
              </p>
            </>
          ) : (
            <p className="text-secondary">{t("startupProblem.noBackups")}</p>
          )}
          <p className="text-secondary">{t("startupProblem.report")}</p>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              onClick={() => {
                showSettings("about");
              }}
            >
              {t("startupProblem.openAbout")}
            </Button>
          </div>
          <details className="text-12 text-tertiary">
            <summary className="w-fit cursor-pointer rounded-control">{t("errors.details")}</summary>
            <pre className="mt-2 font-mono break-words whitespace-pre-wrap select-text">{technical}</pre>
          </details>
        </div>
      </section>
    </div>
  );
}
