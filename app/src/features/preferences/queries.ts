import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc, type Preferences } from "../../services/ipc";

export const preferencesQueryKey = ["preferences"] as const;

/** What the core stores when nothing was chosen yet. Used while loading or after a load error. */
export const DEFAULT_PREFERENCES: Preferences = { theme: "system", language: "system" };

export function usePreferences() {
  return useQuery({ queryKey: preferencesQueryKey, queryFn: ipc.getPreferences });
}

/**
 * Saves preferences through the core. The change is applied to the UI at once (optimistic) and
 * rolled back if the core rejects it; the error stays on the mutation for the UI to show.
 */
export function useSetPreferences() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ipc.setPreferences,
    onMutate: async (next: Preferences) => {
      await queryClient.cancelQueries({ queryKey: preferencesQueryKey });
      const previous = queryClient.getQueryData<Preferences>(preferencesQueryKey);
      queryClient.setQueryData(preferencesQueryKey, next);
      return { previous };
    },
    onError: (_error, _next, context) => {
      queryClient.setQueryData(preferencesQueryKey, context?.previous ?? DEFAULT_PREFERENCES);
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(preferencesQueryKey, saved);
    },
  });
}
