import type { QueryClient } from "@tanstack/react-query";
import type { CoreEvent } from "../../services/ipc";
import { applySettingsSyncStatus } from "./queries";

/**
 * Stage 11: the sync's status. Data it took over arrives with the usual events
 * (`preferencesChanged`, `accountsChanged`, `inboxChanged`), which their own handlers apply.
 */
export function handleSettingsSyncEvent(queryClient: QueryClient, event: CoreEvent): void {
  if (event.type === "settingsSyncChanged") applySettingsSyncStatus(queryClient, event.status);
}
