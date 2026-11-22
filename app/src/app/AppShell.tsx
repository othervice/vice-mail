import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../components/ErrorNotice";
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

  return (
    <div className="flex h-full bg-canvas text-primary">
      <IconRail />
      <main className="flex min-w-0 flex-1 flex-col bg-surface">
        {eventsError && (
          <div className="px-6 pt-4">
            <ErrorNotice context={t("errors.context.events")} error={eventsError} />
          </div>
        )}
        {view.kind === "mail" ? <MailView viewId={view.mailView} /> : <SettingsView section={view.section} />}
      </main>
    </div>
  );
}
