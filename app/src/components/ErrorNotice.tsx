import { CircleExclamationPoint } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { IpcError } from "../services/ipc";
import { Button } from "./Button";

interface ErrorNoticeProps {
  /** What failed, in the words of the place where it is shown. */
  context: string;
  error: IpcError;
  onRetry?: () => void;
  /** Further ways forward next to "Try again", e.g. "Change password". */
  actions?: ReactNode;
  "data-testid"?: string;
}

/**
 * Shows an error from the core where it happened: what failed, a text chosen by the error code
 * with a way forward, and the technical detail behind a disclosure.
 */
export function ErrorNotice({ context, error, onRetry, actions, ...rest }: ErrorNoticeProps) {
  const { t } = useTranslation();
  const technical = [error.code, error.message, error.detail].filter(Boolean).join("\n");
  return (
    <div
      role="alert"
      data-testid={rest["data-testid"]}
      data-error-code={error.code}
      className="flex gap-3 rounded-card border border-danger bg-surface p-4"
    >
      <CircleExclamationPoint aria-hidden className="size-(--ui-icon) shrink-0 text-danger" strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <p className="font-semibold text-primary">{context}</p>
        <p className="text-secondary">{t(`errors.codes.${error.code}.title`)}</p>
        <p className="text-secondary">{t(`errors.codes.${error.code}.hint`)}</p>
        {(onRetry || actions) && (
          <div className="flex flex-wrap gap-2">
            {onRetry && (
              <Button variant="secondary" onClick={onRetry}>
                {t("errors.retry")}
              </Button>
            )}
            {actions}
          </div>
        )}
        <details className="text-12 text-tertiary">
          <summary className="w-fit cursor-pointer rounded-control">{t("errors.details")}</summary>
          <pre className="mt-2 font-mono break-words whitespace-pre-wrap select-text">{technical}</pre>
        </details>
      </div>
    </div>
  );
}
