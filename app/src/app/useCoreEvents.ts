import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { preferencesQueryKey } from "../features/preferences/queries";
import { ipc, toCoreError, type CoreError, type CoreEvent, type Unsubscribe } from "../services/ipc";

function handleCoreEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "preferencesChanged":
      // Another window (or the core itself) changed them; the effects re-apply theme and language.
      queryClient.setQueryData(preferencesQueryKey, event.preferences);
      break;
  }
}

/**
 * Subscribes once to the typed event stream of the core and feeds it into the query cache.
 * Returns the subscription error, if any, so the frame can show it.
 */
export function useCoreEvents(): CoreError | null {
  const queryClient = useQueryClient();
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: Unsubscribe | undefined;
    ipc
      .onCoreEvent((event) => {
        handleCoreEvent(queryClient, event);
      })
      .then(
        (stop) => {
          if (active) unsubscribe = stop;
          else stop();
        },
        (e: unknown) => {
          if (active) setError(toCoreError(e));
        },
      );
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [queryClient]);

  return error;
}
