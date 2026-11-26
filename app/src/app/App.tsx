import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import type { i18n as I18n } from "i18next";
import { Tooltip } from "radix-ui";
import { useState } from "react";
import { I18nextProvider, useTranslation } from "react-i18next";
import { FileX } from "lucide-react";
import { EmptyState } from "../components/EmptyState";
import { useAppInfo, useWindowBackdrop } from "../features/app-info/queries";
import { usePlatformAttribute } from "../features/app-info/usePlatformAttribute";
// Stage 22 (decision 0079): Mica behind the chrome on Windows 11.
import { useBackdropAttribute } from "../features/app-info/useBackdropAttribute";
import { DEFAULT_PREFERENCES, usePreferences } from "../features/preferences/queries";
import { usePreferenceEffects } from "../features/preferences/usePreferenceEffects";
// Stage 8: UI preferences and layout from the core; message and composer windows.
import { useUiPreferencesSync } from "../features/keyboard/useUiPreferencesSync";
import { ComposerWindow } from "../features/windows/ComposerWindow";
import { MessageWindow } from "../features/windows/MessageWindow";
import { currentWindowRoute } from "../features/windows/route";
// Stage 9 (decision 0027): an opened `.eml` file in a window of its own.
import { EmlWindow } from "../features/eml/EmlWindow";
// Stage 19 (decision 0055): a message's source; the window has no event permission.
import { SourceRoot } from "../features/source/SourceWindow";
// Stage 15 (decision 0044): the guided first start of a new installation.
import { FirstStartGate } from "../features/first-start/FirstStartGate";
// Stage 21 (decision 0069): the one-time note when the main window is first closed.
import { CloseHintHost } from "../features/desktop/CloseHint";
import { AppShell } from "./AppShell";
import { useCoreEvents } from "./useCoreEvents";

/** True once the query has answered at all, with data or an error. A later retry keeps it true. */
function hasSettled(query: { dataUpdatedAt: number; errorUpdatedAt: number }): boolean {
  return query.dataUpdatedAt > 0 || query.errorUpdatedAt > 0;
}

/** A window opened with a query the app does not understand: it says so instead of guessing. */
function InvalidWindow() {
  const { t } = useTranslation();
  return (
    <main className="flex h-full flex-col bg-surface text-primary">
      <EmptyState icon={FileX} title={t("windows.invalid.title")} body={t("windows.invalid.body")} />
    </main>
  );
}

function AppRoot() {
  const preferences = usePreferences();
  const appInfo = useAppInfo();
  const eventsError = useCoreEvents();
  // Stage 8: which window this is (the query of the page), its layout and the UI preferences.
  const [route] = useState(currentWindowRoute);
  const ui = useUiPreferencesSync({ migrate: route.kind === "main" });

  // While loading (or after a load error) the defaults apply: follow the system.
  usePreferenceEffects(preferences.data ?? DEFAULT_PREFERENCES);
  usePlatformAttribute(appInfo.data?.platform);
  const backdrop = useWindowBackdrop();
  useBackdropAttribute(backdrop.data);

  // All answers are local and take milliseconds. Waiting for the first one avoids showing the
  // frame in the wrong theme, language, title-bar layout or backdrop. Errors are shown inside
  // the frame (a failed backdrop question just leaves every surface opaque), and a retry from
  // there must not unmount it again.
  if (!hasSettled(preferences) || !hasSettled(appInfo) || !hasSettled(backdrop) || !ui.settled) return null;
  switch (route.kind) {
    case "main":
      return (
        <>
          <FirstStartGate eventsError={eventsError}>
            <AppShell eventsError={eventsError} />
          </FirstStartGate>
          {/* Outside the gate: the window can be closed during the first start too. */}
          <CloseHintHost />
        </>
      );
    case "message":
      return <MessageWindow accountId={route.accountId} messageId={route.messageId} eventsError={eventsError} />;
    case "composer":
      return <ComposerWindow draftId={route.draftId} eventsError={eventsError} />;
    case "eml":
      return <EmlWindow fileId={route.fileId} eventsError={eventsError} />;
    case "source":
      // Routed by `App` before this root; it never subscribes to core events.
      return null;
    case "invalid":
      return <InvalidWindow />;
  }
}

export interface AppProps {
  queryClient: QueryClient;
  i18n: I18n;
}

export function App({ queryClient, i18n }: AppProps) {
  const [route] = useState(currentWindowRoute);
  return (
    <QueryClientProvider client={queryClient}>
      <I18nextProvider i18n={i18n}>
        <Tooltip.Provider delayDuration={400}>
          {/* Stage 19: a source window may not listen to core events (capability `source-window`). */}
          {route.kind === "source" ? <SourceRoot accountId={route.accountId} messageId={route.messageId} /> : <AppRoot />}
        </Tooltip.Provider>
      </I18nextProvider>
    </QueryClientProvider>
  );
}
