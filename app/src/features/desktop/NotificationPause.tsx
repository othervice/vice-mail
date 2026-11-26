import { BellOff } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { useNotificationPause, usePauseNotifications } from "./queries";
import { cx } from "../../components/cx";
import { SETTING_ANCHOR_CLASS, SETTING_BLOCK, settingAnchor } from "../../components/settingAnchor";

/** "14:30" or "2:30 PM", in the UI's language. */
function timeOf(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { hour: "numeric", minute: "2-digit" }).format(ms);
}

/**
 * Settings → Notifications, stage 21 (decision 0069): pause for an hour or until tomorrow
 * morning, or resume. The tray menu offers the same; its changes arrive as
 * `notificationPauseChanged`.
 */
export function NotificationPause({ disabled = false }: { disabled?: boolean }) {
  const { t, i18n } = useTranslation();
  const headingId = useId();
  const pause = useNotificationPause();
  const change = usePauseNotifications();
  const until = pause.data?.until ?? null;
  const busy = change.isPending || !pause.isSuccess;

  let state = null;
  if (until !== null) {
    const later = pause.data?.localTime?.laterDay ?? false;
    state = (
      <p role="status" className="flex items-center gap-2 text-13 text-primary" data-testid="pause-state">
        <BellOff aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
        {t(later ? "desktop.pause.pausedUntilTomorrow" : "desktop.pause.pausedUntil", { time: timeOf(until, i18n.language) })}
      </p>
    );
  }

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3" data-testid="notification-pause" {...SETTING_BLOCK}>
      {/* Stage 22 (decision 0073): found by the settings search. */}
      <div {...settingAnchor(t("desktop.pause.title"))} className={cx("flex flex-col gap-1", SETTING_ANCHOR_CLASS)}>
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {t("desktop.pause.title")}
        </h3>
        <p className="text-13 text-secondary">{t("desktop.pause.description")}</p>
      </div>
      {pause.isError && (
        <ErrorNotice
          context={t("desktop.pause.loadFailed")}
          error={pause.error}
          onRetry={() => {
            void pause.refetch();
          }}
        />
      )}
      {state}
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          disabled={disabled || busy}
          onClick={() => {
            change.mutate("oneHour");
          }}
          data-testid="pause-hour"
        >
          {t("desktop.pause.hour")}
        </Button>
        <Button
          variant="secondary"
          disabled={disabled || busy}
          onClick={() => {
            change.mutate("untilTomorrow");
          }}
          data-testid="pause-tomorrow"
        >
          {t("desktop.pause.tomorrow")}
        </Button>
        {until !== null && (
          <Button
            variant="quiet"
            disabled={busy}
            onClick={() => {
              change.mutate("resume");
            }}
            data-testid="pause-resume"
          >
            {t("desktop.pause.resume")}
          </Button>
        )}
      </div>
      {change.isError && <ErrorNotice context={t("desktop.pause.failed")} error={change.error} />}
    </section>
  );
}
