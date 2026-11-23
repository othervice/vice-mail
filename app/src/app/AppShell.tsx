import { useEffect, useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../components/ErrorNotice";
import { AiPanel } from "../features/ai/AiPanel";
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
import { UpdateNotices } from "../features/updates/UpdateNotices";
import type { CoreError } from "../services/ipc";
import { useUiStore } from "../stores/ui";
import { useSidebarFit } from "../features/layout/useSidebarFit";
import { IconRail } from "./IconRail";
import { SidebarColumn } from "./SidebarColumn";

/** The column stands next to the content (not folded, the window wide enough). */
const isDocked = (state: { sidebarExpanded: boolean; sidebarNarrow: boolean }) => state.sidebarExpanded && !state.sidebarNarrow;

interface AppShellProps {
  eventsError: CoreError | null;
}

/**
 * The window frame: the accounts-and-folders column (folded: the icon rail) on the left, the
 * current view on the right. In a narrow window the column opens over the content.
 */
export function AppShell({ eventsError }: AppShellProps) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const docked = useUiStore(isDocked);
  const overlay = useUiStore((state) => state.sidebarNarrow && state.sidebarOverlay);
  // Stage 9: system notifications speak the UI's language.
  useNotificationTexts();
  useSidebarFit();
  // Folding or unfolding replaces the control that had the focus (the toggle, a row): with the
  // keyboard, the other side's toggle takes it. Not after a click (no tooltip pops up for the
  // mouse) nor when loading the layout changes it.
  const refocus = useRef(false);
  useEffect(() => {
    let keyboard = false;
    const onKey = () => {
      keyboard = true;
    };
    const onPointer = () => {
      keyboard = false;
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onPointer, true);
    const off = useUiStore.subscribe((state, previous) => {
      if (isDocked(state) === isDocked(previous)) return;
      const active = document.activeElement;
      refocus.current =
        keyboard && active instanceof Element && active.closest('[data-testid="sidebar-column"], [data-testid="icon-rail"]') !== null;
    });
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onPointer, true);
      off();
    };
  }, []);
  useLayoutEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    if (document.activeElement === null || document.activeElement === document.body) {
      document.querySelector<HTMLElement>('[data-testid="rail-folders"]')?.focus();
    }
  }, [docked]);

  return (
    // `data-sidebar` lets app.css move header titles clear of the macOS window buttons.
    <div className="relative flex h-full bg-canvas text-primary" data-sidebar={docked ? "expanded" : "collapsed"}>
      {docked ? <SidebarColumn overlay={false} /> : <IconRail />}
      {overlay && <SidebarColumn overlay />}
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
      {/* Stage 13: the AI panel on the right; absent while no AI function is usable. */}
      <AiPanel />
      <Notices />
      <SendNotices />
      {/* Stage 15 (decision 0037): the one-time update question, a quiet note when one is out. */}
      <UpdateNotices />
      {/* Stage 9: mailto: links from the system open the composer. */}
      <MailtoHost />
      {/* Stage 8: command palette, shortcut overview, layout of the window. */}
      <KeyboardLayer />
    </div>
  );
}
