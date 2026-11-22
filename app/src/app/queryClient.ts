import { QueryClient } from "@tanstack/react-query";
import type { CoreError } from "../services/ipc";

declare module "@tanstack/react-query" {
  interface Register {
    // Every query and mutation goes through services/ipc.ts, which only throws CoreError.
    defaultError: CoreError;
  }
}

/**
 * Data from the core. It is local and changes only through commands or core events, so queries
 * never go stale on their own, never poll and never retry: a failed call shows its error instead.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: Infinity,
        retry: false,
        // An error stays until the user retries; mounting another view must not refetch it.
        retryOnMount: false,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      },
      mutations: { retry: false },
    },
  });
}
