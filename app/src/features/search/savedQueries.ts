import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { savedSearchIpc, type RecentSearch, type SavedSearch, type SavedSearchId } from "../../services/ipc";

/** Stage 7: saved searches (synced; `savedSearchesChanged` reloads them) and recent searches (this device). */
export const savedSearchesQueryKey = ["savedSearches"] as const;
export const recentSearchesQueryKey = ["recentSearches"] as const;

/** Saved searches in the order of the sidebar. */
export function useSavedSearches() {
  return useQuery({ queryKey: savedSearchesQueryKey, queryFn: savedSearchIpc.list });
}

/** The recent searches of this device, newest first. Only this window changes them. */
export function useRecentSearches() {
  return useQuery({ queryKey: recentSearchesQueryKey, queryFn: savedSearchIpc.listRecent });
}

/** Remembers a search the user ran; the core answers with the new list. */
export function useAddRecentSearch() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (query: string) => savedSearchIpc.addRecent(query),
    onSuccess: (list: RecentSearch[]) => {
      queryClient.setQueryData(recentSearchesQueryKey, list);
    },
  });
}

export function useClearRecentSearches() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => savedSearchIpc.clearRecent(),
    onSuccess: () => {
      queryClient.setQueryData<RecentSearch[]>(recentSearchesQueryKey, []);
    },
  });
}

/** Puts a saved search into the cached list (sorted like the sidebar) right away. */
function upsert(list: SavedSearch[] | undefined, saved: SavedSearch): SavedSearch[] {
  const others = (list ?? []).filter((s) => s.id !== saved.id);
  return [...others, saved].sort((a, b) => a.position - b.position);
}

export function useSaveSearch() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, query }: { name: string; query: string }) => savedSearchIpc.save(name, query),
    onSuccess: (saved) => {
      queryClient.setQueryData<SavedSearch[]>(savedSearchesQueryKey, (list) => upsert(list, saved));
      void queryClient.invalidateQueries({ queryKey: savedSearchesQueryKey });
    },
  });
}

export function useRenameSavedSearch() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name }: { id: SavedSearchId; name: string }) => savedSearchIpc.rename(id, name),
    onSuccess: (saved) => {
      queryClient.setQueryData<SavedSearch[]>(savedSearchesQueryKey, (list) => upsert(list, saved));
      void queryClient.invalidateQueries({ queryKey: savedSearchesQueryKey });
    },
  });
}

export function useDeleteSavedSearch() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: SavedSearchId) => savedSearchIpc.delete(id),
    onSuccess: (_, id) => {
      queryClient.setQueryData<SavedSearch[]>(savedSearchesQueryKey, (list) => (list ?? []).filter((s) => s.id !== id));
      void queryClient.invalidateQueries({ queryKey: savedSearchesQueryKey });
    },
  });
}
