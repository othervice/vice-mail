import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import type { i18n as I18n } from "i18next";
import { Tooltip } from "radix-ui";
import { I18nextProvider } from "react-i18next";
import { useAppInfo } from "../features/app-info/queries";
import { usePlatformAttribute } from "../features/app-info/usePlatformAttribute";
import { DEFAULT_PREFERENCES, usePreferences } from "../features/preferences/queries";
import { usePreferenceEffects } from "../features/preferences/usePreferenceEffects";
import { AppShell } from "./AppShell";
import { useCoreEvents } from "./useCoreEvents";

/** True once the query has answered at all, with data or an error. A later retry keeps it true. */
function hasSettled(query: { dataUpdatedAt: number; errorUpdatedAt: number }): boolean {
  return query.dataUpdatedAt > 0 || query.errorUpdatedAt > 0;
}

function AppRoot() {
  const preferences = usePreferences();
  const appInfo = useAppInfo();
  const eventsError = useCoreEvents();

  // While loading (or after a load error) the defaults apply: follow the system.
  usePreferenceEffects(preferences.data ?? DEFAULT_PREFERENCES);
  usePlatformAttribute(appInfo.data?.platform);

  // Both answers are local and take milliseconds. Waiting for the first one avoids showing the
  // frame in the wrong theme, language or title-bar layout. Errors are shown inside the frame,
  // and a retry from there must not unmount it again.
  if (!hasSettled(preferences) || !hasSettled(appInfo)) return null;
  return <AppShell eventsError={eventsError} />;
}

export interface AppProps {
  queryClient: QueryClient;
  i18n: I18n;
}

export function App({ queryClient, i18n }: AppProps) {
  return (
    <QueryClientProvider client={queryClient}>
      <I18nextProvider i18n={i18n}>
        <Tooltip.Provider delayDuration={400}>
          <AppRoot />
        </Tooltip.Provider>
      </I18nextProvider>
    </QueryClientProvider>
  );
}
