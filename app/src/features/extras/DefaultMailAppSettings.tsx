import { CircleCheck, Mail } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { PRODUCT_NAME } from "../../config/product";
import type { DefaultMailApp } from "../../services/ipc";
import { useDefaultMailApp, useSetDefaultMailApp } from "./queries";

function stateKey(app: DefaultMailApp): "yes" | "no" | "unknown" {
  if (app.isDefault === null) return "unknown";
  return app.isDefault ? "yes" : "no";
}

/**
 * Settings → General: whether `mailto:` links open this app, and the way to change that which
 * the system offers. macOS lets the app make itself the default (`direct`); Windows keeps the
 * choice to the user, so the button opens its settings and the app cannot tell the result.
 * When the direct way fails (a build that is not installed), the system's own way is explained.
 */
export function DefaultMailAppSettings() {
  const { t } = useTranslation();
  const status = useDefaultMailApp();
  const set = useSetDefaultMailApp();
  const headingId = useId();
  const product = PRODUCT_NAME;

  let body;
  if (status.isPending) {
    body = (
      <p role="status" className="text-13 text-secondary">
        {t("extras.defaultApp.checking")}
      </p>
    );
  } else if (status.isError) {
    body = (
      <ErrorNotice
        context={t("extras.defaultApp.loadFailed", { product })}
        error={status.error}
        onRetry={() => {
          void status.refetch();
        }}
        data-testid="default-app-error"
      />
    );
  } else {
    const app = status.data;
    const state = stateKey(app);
    body = (
      <>
        <p className="flex items-center gap-2 text-13 text-primary" data-testid="default-app-state" data-state={state}>
          {state === "yes" ? (
            <CircleCheck aria-hidden className="size-(--ui-icon-small) shrink-0 text-success" strokeWidth={1.75} />
          ) : (
            <Mail aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
          )}
          {t(`extras.defaultApp.state.${state}`, { product })}
        </p>
        {state !== "yes" && (
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              disabled={set.isPending}
              onClick={() => {
                set.mutate();
              }}
              data-testid="default-app-set"
            >
              {app.direct ? t("extras.defaultApp.makeDefault", { product }) : t("extras.defaultApp.openSettings")}
            </Button>
            {/* After a change in the system's own settings (macOS: Mail); Windows never tells. */}
            {state === "no" && (
              <Button
                variant="quiet"
                disabled={status.isFetching}
                onClick={() => {
                  void status.refetch();
                }}
                data-testid="default-app-recheck"
              >
                {t("extras.defaultApp.recheck")}
              </Button>
            )}
          </div>
        )}
        {set.isSuccess && (
          <Notice
            tone={set.data === "manual" ? "warning" : set.data === "done" ? "success" : "info"}
            role="status"
            title={t(`extras.defaultApp.outcome.${set.data}`, { product })}
            data-testid="default-app-outcome"
          />
        )}
        {set.isError && <ErrorNotice context={t("extras.defaultApp.setFailed", { product })} error={set.error} />}
      </>
    );
  }

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3" data-testid="default-app">
      <div className="flex flex-col gap-1">
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {t("extras.defaultApp.title")}
        </h3>
        <p className="text-13 text-secondary">{t("extras.defaultApp.description", { product })}</p>
      </div>
      {body}
    </section>
  );
}
