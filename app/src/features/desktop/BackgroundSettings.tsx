import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Switch } from "../../components/Switch";
import { PRODUCT_NAME } from "../../config/product";
import { useAppInfo } from "../app-info/queries";
import {
  BACKGROUND_DEFAULTS,
  useAutostart,
  useBackgroundSettings,
  useSetAutostart,
  useSetBackgroundSettings,
} from "./queries";

/**
 * Settings → General, stage 21 (decisions 0069, 0070): whether closing the window keeps the app
 * running, the macOS menu-bar icon (Windows always shows its icon in the notification area, so
 * a note says so), and starting with the system, which the system's own registration answers.
 */
export function BackgroundSettings() {
  const { t } = useTranslation();
  const headingId = useId();
  const product = PRODUCT_NAME;
  const platform = useAppInfo().data?.platform;
  const settings = useBackgroundSettings();
  const save = useSetBackgroundSettings();
  const autostart = useAutostart();
  const setAutostart = useSetAutostart();
  const current = { ...BACKGROUND_DEFAULTS, ...settings.data };
  const ready = settings.isSuccess && !save.isPending;
  const macos = platform === "macos";

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-4" data-testid="background-settings">
      <h3 id={headingId} className="text-14 font-semibold text-primary">
        {t("desktop.background.title")}
      </h3>
      {settings.isError && (
        <ErrorNotice
          context={t("desktop.background.loadFailed")}
          error={settings.error}
          onRetry={() => {
            void settings.refetch();
          }}
        />
      )}
      <Switch
        label={t("desktop.background.keepRunning.label")}
        description={t(macos ? "desktop.background.keepRunning.macos" : "desktop.background.keepRunning.windows", { product })}
        checked={current.keepRunning}
        disabled={!ready}
        onChange={(keepRunning) => {
          save.mutate({ ...current, keepRunning });
        }}
        data-testid="background-keep-running"
      />
      {macos ? (
        <Switch
          label={t("desktop.background.menuBarIcon.label")}
          description={t("desktop.background.menuBarIcon.description", { product })}
          checked={current.menuBarIcon}
          disabled={!ready}
          onChange={(menuBarIcon) => {
            save.mutate({ ...current, menuBarIcon });
          }}
          data-testid="background-menu-bar-icon"
        />
      ) : (
        <p className="text-13 text-secondary" data-testid="background-tray-note">
          {t("desktop.background.trayNote", { product })}
        </p>
      )}
      {save.isError && <ErrorNotice context={t("desktop.background.saveFailed")} error={save.error} />}
      <Switch
        label={t("desktop.background.autostart.label")}
        description={
          autostart.data && !autostart.data.available
            ? t("desktop.background.autostart.unavailable")
            : t("desktop.background.autostart.description", { product })
        }
        checked={autostart.data?.enabled ?? false}
        disabled={!autostart.data?.available || setAutostart.isPending}
        onChange={(enabled) => {
          setAutostart.mutate(enabled);
        }}
        data-testid="background-autostart"
      />
      {setAutostart.isError && (
        <ErrorNotice context={t("desktop.background.autostart.failed", { product })} error={setAutostart.error} />
      )}
    </section>
  );
}
