import { BookUser } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { SettingsSection } from "../../components/SettingsSection";
import { useUiStore } from "../../stores/ui";
import { ContactDuplicates } from "./ContactDuplicates";

/**
 * Settings → Contacts. Since stage 17 (decision 0050) the directory is a view of its own
 * ("Contacts" in the column, the rail and the palette): this section links to it and keeps the
 * duplicate suggestions, which are merged only after an explicit confirmation (stage 10).
 */
export function ContactsSettings() {
  const { t } = useTranslation();
  const showMailView = useUiStore((state) => state.showMailView);
  return (
    <SettingsSection title={t("settings.contacts.title")}>
      <p className="text-13 text-secondary">{t("contacts.intro")}</p>
      <div>
        <Button
          variant="secondary"
          onClick={() => {
            showMailView("contacts");
          }}
          data-testid="contacts-open-view"
        >
          <BookUser aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("contacts.openView")}
        </Button>
      </div>
      <ContactDuplicates />
    </SettingsSection>
  );
}
