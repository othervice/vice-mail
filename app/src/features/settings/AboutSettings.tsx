import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { AppIcon, Wordmark } from "../../components/Brand";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { LICENSE_SPDX, PRODUCT_NAME, PROJECT_URL } from "../../config/product";
import { useAppInfo } from "../app-info/queries";
import { DiagnosticsPanel } from "../diagnostics/DiagnosticsPanel";
import { UpdatePanel } from "../updates/UpdatePanel";
import { CopyableAddress } from "./CopyableAddress";
import { OpenSourceLicenses } from "./OpenSourceLicenses";

function formatBuildDate(value: string, language: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(language, { dateStyle: "long", timeStyle: "short" }).format(date);
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-secondary">{label}</dt>
      <dd className="min-w-0 text-primary select-text">{children}</dd>
    </>
  );
}

export function AboutSettings() {
  const { t, i18n } = useTranslation();
  const appInfo = useAppInfo();
  const info = appInfo.data;
  const unknown = t("settings.about.unknown");

  return (
    <SettingsSection title={t("settings.about.title")}>
      {/* Design pass 23.11.2026: the app icon and the wordmark from docs/brand, then the facts. */}
      <div
        className="flex items-center gap-6 rounded-card border border-border-subtle bg-surface-raised p-6 shadow-card"
        data-testid="about-brand"
      >
        <AppIcon className="size-(--ui-app-icon) shrink-0" />
        <div className="flex min-w-0 flex-col items-start gap-2">
          <p className="flex">
            <span className="sr-only">{PRODUCT_NAME}</span>
            <Wordmark lettering className="h-(--ui-lettering-height) w-auto max-w-full text-brand-ink" />
          </p>
          <p className="text-14 text-secondary">{t("settings.about.tagline")}</p>
        </div>
      </div>
      {appInfo.isError && (
        <ErrorNotice
          context={t("errors.context.appInfo")}
          error={appInfo.error}
          onRetry={() => {
            void appInfo.refetch();
          }}
        />
      )}
      <dl className="grid grid-cols-[auto_1fr] items-baseline gap-x-8 gap-y-3 text-14">
        {info && (
          <>
            <Row label={t("settings.about.version")}>{info.version}</Row>
            <Row label={t("settings.about.commit")}>
              {info.commit ? <span className="font-mono">{info.commit}</span> : unknown}
            </Row>
            <Row label={t("settings.about.buildDate")}>
              {info.buildDate ? formatBuildDate(info.buildDate, i18n.language) : unknown}
            </Row>
            <Row label={t("settings.about.platform")}>{t(`platforms.${info.platform}`)}</Row>
          </>
        )}
        <Row label={t("settings.about.license")}>{LICENSE_SPDX}</Row>
        <Row label={t("settings.about.project")}>
          <CopyableAddress url={PROJECT_URL} data-testid="about-copy-project" />
        </Row>
      </dl>
      {/* Stage 15 (decision 0037): updates, opt-in. */}
      <UpdatePanel />
      {/* Stage 15: "Export diagnostics…", a file the user reads before sending it. */}
      <DiagnosticsPanel />
      {/* Stage 15: the third-party libraries and their licences (scripts/licenses.mjs). */}
      <OpenSourceLicenses />
    </SettingsSection>
  );
}
