import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { LICENSE_SPDX, PRODUCT_NAME } from "../../config/product";
import { useAppInfo } from "../app-info/queries";

function formatBuildDate(value: string, language: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(language, { dateStyle: "long", timeStyle: "short" }).format(date);
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-secondary">{label}</dt>
      <dd className="text-primary select-text">{children}</dd>
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
      <p className="text-20 font-semibold text-primary">{PRODUCT_NAME}</p>
      {appInfo.isError && (
        <ErrorNotice
          context={t("errors.context.appInfo")}
          error={appInfo.error}
          onRetry={() => {
            void appInfo.refetch();
          }}
        />
      )}
      <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-14">
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
      </dl>
    </SettingsSection>
  );
}
