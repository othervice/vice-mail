import { useTranslation } from "react-i18next";
import { ChoiceGroup, type ChoiceOption } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import type { ThemePreference } from "../../services/ipc";
import { LayoutSettings } from "../layout/LayoutSettings";
import { DEFAULT_PREFERENCES, usePreferences, useSetPreferences } from "../preferences/queries";
import { AccentPicker } from "./AccentPicker";

export function AppearanceSettings() {
  const { t } = useTranslation();
  const preferences = usePreferences();
  const save = useSetPreferences();
  const current = preferences.data ?? DEFAULT_PREFERENCES;

  const options: ChoiceOption<ThemePreference>[] = [
    { value: "system", label: t("settings.appearance.theme.system") },
    { value: "light", label: t("settings.appearance.theme.light") },
    { value: "dark", label: t("settings.appearance.theme.dark") },
  ];

  return (
    <SettingsSection title={t("settings.appearance.title")}>
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
        label={t("settings.appearance.theme.label")}
        description={t("settings.appearance.theme.description")}
        value={current.theme}
        options={options}
        onChange={(theme) => {
          save.mutate({ ...current, theme });
        }}
      />
      {save.isError && <ErrorNotice context={t("errors.context.savePreferences")} error={save.error} />}
      {/* Stage 16: the accent theme (per device until the core stores it). */}
      <AccentPicker />
      {/* Stage 8: layout of this window. */}
      <LayoutSettings />
    </SettingsSection>
  );
}
