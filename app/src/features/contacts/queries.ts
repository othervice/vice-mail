import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { contactsIpc, type ContactMergeRequest } from "../../services/ipc";

// Stage 10: the local contact directory (decision 0023). The core sends no event for it: the
// views re-query after their own commands.

/** Contacts per page of the directory (`listContacts` takes 1..200). */
export const CONTACT_PAGE_SIZE = 50;
/** Wait after the last key before searching the directory. */
export const CONTACT_SEARCH_DEBOUNCE_MS = 200;

export const contactDirectoryQueryKey = (query: string) => ["contactDirectory", query] as const;
export const contactDuplicatesQueryKey = ["contactDuplicates"] as const;

/** The query once typing paused (trimmed). */
export function useDebouncedQuery(value: string, delay = CONTACT_SEARCH_DEBOUNCE_MS): string {
  const [debounced, setDebounced] = useState(value.trim());
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(value.trim());
    }, delay);
    return () => {
      window.clearTimeout(timer);
    };
  }, [value, delay]);
  return debounced;
}

/** The ranked directory, page by page; while the user types, the last results stay. */
export function useContactDirectory(query: string) {
  return useInfiniteQuery({
    queryKey: contactDirectoryQueryKey(query),
    queryFn: ({ pageParam }) => contactsIpc.listContacts(query === "" ? null : query, pageParam, CONTACT_PAGE_SIZE),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.next,
    placeholderData: keepPreviousData,
  });
}

export function useContactDuplicates() {
  return useQuery({ queryKey: contactDuplicatesQueryKey, queryFn: contactsIpc.listContactDuplicates });
}

/** After a change: the directory, the suggestions and the composer's autocomplete. */
function refreshContacts(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: ["contactDirectory"] });
  void queryClient.invalidateQueries({ queryKey: contactDuplicatesQueryKey });
  void queryClient.invalidateQueries({ queryKey: ["contacts"] });
}

export function useSetContactName() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ email, name }: { email: string; name: string | null }) => contactsIpc.setContactName(email, name),
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}

export function useMergeContacts() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: ContactMergeRequest) => contactsIpc.mergeContacts(request),
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}

export function useDismissDuplicate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (keys: string[]) => contactsIpc.dismissContactDuplicate(keys),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: contactDuplicatesQueryKey });
    },
  });
}

export function useSeparateContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (email: string) => contactsIpc.separateContact(email),
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}
