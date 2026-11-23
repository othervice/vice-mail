import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { Notice } from "../../components/Notice";
import { useUiStore } from "../../stores/ui";
import { useAccounts } from "../accounts/queries";
import { useSettingsSyncStatus } from "./queries";

/**
 * On a second device: once the mail sync of a new account sees a settings sync folder, the
 * accounts page points to joining it. Shown only while the sync is off here; it says nothing
 * when the status cannot be loaded (the settings sync page shows that error).
 */
export function SyncDetectedHint() {
  const { t } = useTranslation();
  const status = useSettingsSyncStatus();
  const accounts = useAccounts();
  const showSettings = useUiStore((state) => state.showSettings);
  const current = status.data;
  if (current?.state !== "off") return null;
  const account = accounts.data?.find((a) => current.detectedAccounts.includes(a.id));
  if (!account) return null;
  return (
    <Notice tone="info" title={t("settingsSync.off.detectedTitle")} data-testid="sync-detected-hint">
      <p>{t("settingsSync.off.detectedBody", { email: account.email })}</p>
      <div>
        <Button
          variant="secondary"
          onClick={() => {
            showSettings("sync");
          }}
        >
          {t("settingsSync.off.openSync")}
        </Button>
      </div>
    </Notice>
  );
}
