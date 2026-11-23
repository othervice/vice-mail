import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { useAccountGroups } from "../unified/queries";
import { useFolderName } from "./folders";
import { useOtherAccountFolders } from "./transfer";
import { useSetUnreadFirstView, useUnreadFirstViews } from "./unread";

/**
 * Stage 16, Settings → Inbox: the views that show their unread mail first (a device setting of
 * the core), each by its name, with "Turn off". Turning it on happens in the view's header menu.
 */
export function UnreadFirstSettings() {
  const { t } = useTranslation();
  const views = useUnreadFirstViews();
  const set = useSetUnreadFirstView();
  const folders = useOtherAccountFolders("");
  const groups = useAccountGroups(views.data?.some((view) => view.startsWith("group:")) ?? false).data ?? [];
  const folderName = useFolderName();

  const nameOf = (view: string): string => {
    if (view === "unified:all") return t("unified.title");
    if (view.startsWith("group:")) {
      return groups.find((group) => `group:${group.id}` === view)?.name ?? t("organize.unread.settings.unknownGroup");
    }
    for (const { account, mailboxes } of folders) {
      const mailbox = mailboxes.find((candidate) => `mailbox:${String(candidate.id)}` === view);
      if (mailbox) {
        return t("organize.unread.settings.folder", { folder: folderName(mailbox), account: account.displayName || account.email });
      }
    }
    return t("organize.unread.settings.unknownFolder");
  };

  return (
    <SettingsSection sub title={t("organize.unread.settings.title")}>
      <p className="text-13 text-secondary">{t("organize.unread.settings.hint")}</p>
      {views.isError && (
        <ErrorNotice
          context={t("organize.unread.loadFailed")}
          error={views.error}
          onRetry={() => {
            void views.refetch();
          }}
        />
      )}
      {views.data && views.data.length === 0 && (
        <p className="text-13 text-tertiary" data-testid="unread-first-none">
          {t("organize.unread.settings.none")}
        </p>
      )}
      {views.data && views.data.length > 0 && (
        <ul className="flex flex-col divide-y divide-border-subtle rounded-card border border-border-subtle" data-testid="unread-first-views">
          {views.data.map((view) => {
            const name = nameOf(view);
            return (
              <li key={view} className="flex items-center justify-between gap-3 px-3 py-2">
                <span className="min-w-0 truncate text-14 text-primary">{name}</span>
                <Button
                  variant="quiet"
                  aria-label={t("organize.unread.settings.turnOffFor", { name })}
                  disabled={set.isPending}
                  onClick={() => {
                    set.mutate({ view, enabled: false });
                  }}
                  data-testid="unread-first-off"
                >
                  {t("organize.unread.settings.turnOff")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </SettingsSection>
  );
}
