import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { toCoreError } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useUpdateInfo, useUpdateStep } from "./queries";

const CARD =
  "pointer-events-auto flex w-(--ui-notice-width) max-w-full flex-col gap-3 rounded-card border border-border bg-surface-raised p-4 text-14 shadow-floating";

/**
 * Stage 15 (decision 0037), in the main window: the one-time question about automatic update
 * checks (after the first account is set up; not a dialog, nothing waits for it), and a quiet
 * note when an update is available or ready. Neither downloads or installs anything. Since
 * decision 0044 the guided first start asks the question on its optional step; this card is the
 * fallback for anyone who never saw that step (the core no longer asks once it is answered).
 */
export function UpdateNotices() {
  const query = useUpdateInfo();
  const info = query.data;
  const view = useUiStore((state) => state.view);
  const [dismissed, setDismissed] = useState<string | null>(null);
  if (!info?.supported) return null;

  let content = null;
  if (info.askConsent) {
    content = <ConsentQuestion />;
  } else {
    const status = info.status;
    const version =
      status.state === "available" ? status.update.version : status.state === "ready" ? status.version : null;
    const onAbout = view.kind === "settings" && view.section === "about";
    if (version !== null && dismissed !== `${status.state}:${version}` && !onAbout) {
      content = (
        <UpdateNote
          ready={status.state === "ready"}
          version={version}
          onDismiss={() => {
            setDismissed(`${status.state}:${version}`);
          }}
        />
      );
    }
  }
  if (!content) return null;
  return <div className="pointer-events-none fixed start-20 bottom-6 z-30 flex flex-col gap-2">{content}</div>;
}

function ConsentQuestion() {
  const { t } = useTranslation();
  const titleId = useId();
  const answer = useUpdateStep("setAutoCheck");
  return (
    <section aria-labelledby={titleId} className={CARD} data-testid="update-consent">
      <h2 id={titleId} className="font-semibold text-primary">
        {t("updates.consent.title")}
      </h2>
      <p className="text-secondary">{t("updates.consent.body")}</p>
      <p className="text-secondary">{t("updates.consent.noInstall")}</p>
      {answer.isError && <ErrorNotice context={t("errors.context.updateSetting")} error={toCoreError(answer.error)} />}
      <div className="flex flex-wrap justify-end gap-2">
        <Button
          variant="secondary"
          disabled={answer.isPending}
          onClick={() => {
            answer.mutate(false);
          }}
          data-testid="update-consent-no"
        >
          {t("updates.consent.no")}
        </Button>
        <Button
          disabled={answer.isPending}
          onClick={() => {
            answer.mutate(true);
          }}
          data-testid="update-consent-yes"
        >
          {t("updates.consent.yes")}
        </Button>
      </div>
    </section>
  );
}

function UpdateNote({ ready, version, onDismiss }: { ready: boolean; version: string; onDismiss: () => void }) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  return (
    <section aria-label={t("updates.notice.region")} className={CARD} data-testid="update-notice">
      <p role="status" className="font-medium text-primary">
        {ready ? t("updates.notice.ready", { version }) : t("updates.notice.available", { version })}
      </p>
      <div className="flex flex-wrap justify-end gap-2">
        <Button variant="quiet" onClick={onDismiss}>
          {t("updates.notice.dismiss")}
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            showSettings("about");
          }}
          data-testid="update-notice-show"
        >
          {t("updates.notice.show")}
        </Button>
      </div>
    </section>
  );
}
