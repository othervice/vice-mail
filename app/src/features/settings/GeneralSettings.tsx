import { useTranslation } from "react-i18next";
import { ChoiceGroup, type ChoiceOption } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import type { LanguagePreference } from "../../services/ipc";
import { DefaultMailAppSettings } from "../extras/DefaultMailAppSettings";
import { DEFAULT_PREFERENCES, usePreferences, useSetPreferences } from "../preferences/queries";

export function GeneralSettings() {
  const { t } = useTranslation();
  const preferences = usePreferences();
  const save = useSetPreferences();
  const current = preferences.data ?? DEFAULT_PREFERENCES;

  // Language names are shown in their own language, so they stay findable in any UI language.
  const options: ChoiceOption<LanguagePreference>[] = [
    { value: "system", label: t("settings.general.language.system") },
    { value: "en", label: t("languages.en"), lang: "en" },
    { value: "de", label: t("languages.de"), lang: "de" },
  ];

  return (
    <SettingsSection title={t("settings.general.title")}>
      {preferences.isError && (
        <ErrorNotice
          context={t("errors.context.preferences")}
          error={preferences.error}
          onRetry={() => {
            void preferences.refetch();
          }}
        />
      )}
      <ChoiceGroup
        label={t("settings.general.language.label")}
        description={t("settings.general.language.description")}
        value={current.language}
        options={options}
        onChange={(language) => {
          save.mutate({ ...current, language });
        }}
      />
      {save.isError && <ErrorNotice context={t("errors.context.savePreferences")} error={save.error} />}
      {/* Stage 9: mailto: links open this app. */}
      <DefaultMailAppSettings />
    </SettingsSection>
  );
}
