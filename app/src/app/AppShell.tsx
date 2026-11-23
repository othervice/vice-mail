import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../components/ErrorNotice";
import { Notices } from "../features/actions/Notices";
import { useNotificationTexts } from "../features/comfort/events";
import { ActivityView } from "../features/activity/ActivityView";
import { ComposerHost } from "../features/composer/Composer";
import { OutboxView } from "../features/composer/OutboxView";
import { SendNotices } from "../features/composer/SendNotices";
import { MailView } from "../features/mail/MailView";
import { SettingsView } from "../features/settings/SettingsView";
import type { CoreError } from "../services/ipc";
import { useUiStore } from "../stores/ui";
import { IconRail } from "./IconRail";

interface AppShellProps {
  eventsError: CoreError | null;
}

/** The window frame: icon rail on the left, the current view on the right. */
export function AppShell({ eventsError }: AppShellProps) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const sidebarExpanded = useUiStore((state) => state.sidebarExpanded);
  // Stage 9: system notifications speak the UI's language.
  useNotificationTexts();

  return (
    // `data-sidebar` lets app.css move header titles clear of the macOS window buttons.
    <div className="flex h-full bg-canvas text-primary" data-sidebar={sidebarExpanded ? "expanded" : "collapsed"}>
      <IconRail />
      <main className="relative flex min-w-0 flex-1 flex-col bg-surface">
        {eventsError && (
          <div className="px-6 pt-4">
            <ErrorNotice context={t("errors.context.events")} error={eventsError} />
          </div>
        )}
        {view.kind === "settings" ? (
          <SettingsView section={view.section} />
        ) : view.kind === "activity" ? (
          <ActivityView />
        ) : view.kind === "outbox" ? (
          <OutboxView />
        ) : (
          <MailView view={view} />
        )}
        {/* Stage 5: the composer lies over the reading pane; send states at the bottom right. */}
        <ComposerHost />
      </main>
      <Notices />
      <SendNotices />
    </div>
  );
}
