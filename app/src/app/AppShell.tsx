import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../components/ErrorNotice";
import { Notices } from "../features/actions/Notices";
import { useNotificationTexts } from "../features/comfort/events";
import { ActivityView } from "../features/activity/ActivityView";
import { ComposerHost } from "../features/composer/Composer";
import { OutboxView } from "../features/composer/OutboxView";
import { SendNotices } from "../features/composer/SendNotices";
import { MailtoHost } from "../features/extras/MailtoHost";
import { MailView } from "../features/mail/MailView";
import { KeyboardLayer } from "../features/palette/KeyboardLayer";
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
      {/* Focusable (not in the Tab order): where the focus goes when what had it is gone (stage 8). */}
      <main tabIndex={-1} className="relative flex min-w-0 flex-1 flex-col bg-surface outline-none">
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
      {/* Stage 9: mailto: links from the system open the composer. */}
      <MailtoHost />
      {/* Stage 8: command palette, shortcut overview, layout of the window. */}
      <KeyboardLayer />
    </div>
  );
}
