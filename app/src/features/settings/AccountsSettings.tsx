import { UserPlus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { SettingsSection } from "../../components/SettingsSection";

/** Account setup arrives in stage 2. Until then this says so instead of showing a form. */
export function AccountsSettings() {
  const { t } = useTranslation();
  return (
    <SettingsSection title={t("settings.accounts.title")}>
      <div className="flex rounded-card border border-border bg-canvas">
        <EmptyState
          icon={UserPlus}
          title={t("settings.accounts.empty.title")}
          body={t("settings.accounts.empty.body")}
        />
      </div>
    </SettingsSection>
  );
}
