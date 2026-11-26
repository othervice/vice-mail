import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Switch } from "../../components/Switch";
import { toCoreError, type Platform, type UpdateInfo, type UpdateStatus } from "../../services/ipc";
import { useAppInfo } from "../app-info/queries";
import { useUpdateInfo, useUpdateStep } from "./queries";
import { cx } from "../../components/cx";
import { SETTING_ANCHOR_CLASS, SETTING_BLOCK, settingAnchor } from "../../components/settingAnchor";

function formatTime(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(ms));
}

function formatDate(value: string, language: string): string | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(language, { dateStyle: "long" }).format(date);
}

/**
 * Stage 15 (decision 0037), part of Settings → About: the switch for automatic checks with what
 * a check reveals, "Check now", and the state with the one next step (download, restart to
 * install). Nothing happens without a click; release notes are shown as plain text.
 */
export function UpdatePanel() {
  const { t } = useTranslation();
  const query = useUpdateInfo();
  const setAutoCheck = useUpdateStep("setAutoCheck");
  const checkNow = useUpdateStep("checkNow");
  const info = query.data;

  if (query.isError) {
    return (
      <ErrorNotice
        context={t("errors.context.updateInfo")}
        error={query.error}
        onRetry={() => {
          void query.refetch();
        }}
      />
    );
  }
  if (!info) return null;

  return (
    <section aria-labelledby="updates-heading" className="flex flex-col gap-4" data-testid="updates-panel" {...SETTING_BLOCK}>
      {/* Stage 22 (decision 0073): found by the settings search, like its switch. */}
      <h3 id="updates-heading" {...settingAnchor(t("updates.title"))} className={cx("text-14 font-semibold text-primary", SETTING_ANCHOR_CLASS)}>
        {t("updates.title")}
      </h3>
      {info.supported ? (
        <>
          <Switch
            label={t("updates.autoCheck")}
            description={t("updates.autoCheckHint")}
            checked={info.autoCheck}
            disabled={setAutoCheck.isPending}
            onChange={(next) => {
              setAutoCheck.mutate(next);
            }}
            data-testid="updates-auto-check"
          />
          {setAutoCheck.isError && (
            <ErrorNotice context={t("errors.context.updateSetting")} error={toCoreError(setAutoCheck.error)} />
          )}
          <UpdateState info={info} />
          <div className="flex flex-wrap items-center gap-3">
            <Button
              variant="secondary"
              disabled={busy(info.status) || checkNow.isPending}
              onClick={() => {
                checkNow.mutate(undefined);
              }}
              data-testid="updates-check-now"
            >
              {t("updates.checkNow")}
            </Button>
            <span className="text-13 text-secondary">{t("updates.manualHint")}</span>
          </div>
          {checkNow.isError && (
            <ErrorNotice context={t("errors.context.updateAction")} error={toCoreError(checkNow.error)} />
          )}
        </>
      ) : (
        <p className="text-14 text-secondary">{t("updates.unsupported")}</p>
      )}
    </section>
  );
}

function busy(status: UpdateStatus): boolean {
  return status.state === "checking" || status.state === "downloading" || status.state === "installing";
}

function UpdateState({ info }: { info: UpdateInfo }) {
  const { t, i18n } = useTranslation();
  const platform = useAppInfo().data?.platform;
  const download = useUpdateStep("download");
  const install = useUpdateStep("install");
  const status = info.status;
  const lastChecked =
    info.lastCheckMs === null
      ? t("updates.neverChecked")
      : t("updates.lastChecked", { time: formatTime(info.lastCheckMs, i18n.language) });
  const stepError = download.error ?? install.error;
  const published =
    status.state === "available" && status.update.pubDate ? formatDate(status.update.pubDate, i18n.language) : null;

  return (
    <div className="flex flex-col gap-3 text-14" data-testid="updates-state" data-state={status.state}>
      {status.state === "idle" && <p className="text-secondary">{lastChecked}</p>}
      {status.state === "checking" && (
        <p role="status" className="text-secondary">
          {t("updates.checking")}
        </p>
      )}
      {status.state === "upToDate" && (
        <p role="status" className="text-secondary">
          {t("updates.upToDate")} {lastChecked}
        </p>
      )}
      {status.state === "available" && (
        <>
          <p role="status" className="font-medium text-primary">
            {t("updates.available", { version: status.update.version })}
          </p>
          {published !== null && <p className="text-secondary">{t("updates.published", { date: published })}</p>}
          {status.update.notes && (
            <div className="flex flex-col gap-1">
              <p className="text-13 font-medium text-secondary">{t("updates.notes")}</p>
              {/* Plain text from the manifest: never parsed as HTML or Markdown. */}
              <p className="whitespace-pre-wrap text-primary select-text" data-testid="updates-notes">
                {status.update.notes}
              </p>
            </div>
          )}
          <div>
            <Button
              disabled={download.isPending}
              onClick={() => {
                download.mutate(undefined);
              }}
              data-testid="updates-download"
            >
              {t("updates.download")}
            </Button>
          </div>
        </>
      )}
      {status.state === "downloading" && <Progress status={status} />}
      {status.state === "ready" && (
        <>
          <p role="status" className="font-medium text-primary">
            {t("updates.ready", { version: status.version })}
          </p>
          <p className="text-secondary">{t(`updates.restartHint.${restartPlatform(platform)}`)}</p>
          <div>
            <Button
              disabled={install.isPending}
              onClick={() => {
                install.mutate(undefined);
              }}
              data-testid="updates-install"
            >
              {t("updates.restart")}
            </Button>
          </div>
        </>
      )}
      {status.state === "installing" && (
        <p role="status" className="text-secondary">
          {t("updates.installing", { version: status.version })}
        </p>
      )}
      {status.state === "failed" && (
        <ErrorNotice
          context={
            status.version === null
              ? t("errors.context.updateCheck")
              : t("errors.context.updateStep", { version: status.version })
          }
          error={status.error}
          // A found update downloads again; otherwise "Check now" below asks again.
          {...(status.version !== null &&
            status.error.code !== "update_install_failed" && {
              onRetry: () => {
                download.mutate(undefined);
              },
            })}
          data-testid="updates-failed"
        />
      )}
      {stepError && <ErrorNotice context={t("errors.context.updateAction")} error={toCoreError(stepError)} />}
    </div>
  );
}

function restartPlatform(platform: Platform | undefined): "macos" | "windows" {
  return platform === "windows" ? "windows" : "macos";
}

function Progress({ status }: { status: Extract<UpdateStatus, { state: "downloading" }> }) {
  const { t } = useTranslation();
  const percent =
    status.total !== null && status.total > 0 ? Math.min(100, Math.floor((status.downloaded * 100) / status.total)) : null;
  return (
    <div className="flex flex-col gap-2">
      <p role="status" className="text-secondary">
        {percent === null
          ? t("updates.downloading", { version: status.version })
          : t("updates.downloadingPercent", { version: status.version, percent })}
      </p>
      {/* Native, so no inline style is needed; without a total it shows as indeterminate. */}
      <progress
        aria-label={t("updates.progress")}
        max={100}
        value={percent ?? undefined}
        className="h-2 w-full max-w-(--ui-meter-width) appearance-none overflow-hidden rounded-full bg-hover accent-accent [&::-moz-progress-bar]:bg-accent [&::-webkit-progress-bar]:bg-hover [&::-webkit-progress-value]:bg-accent"
      />
    </div>
  );
}
