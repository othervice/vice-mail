import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { contactsIpc, type ContactFilter, type ContactMergeRequest, type CoreEvent } from "../../services/ipc";

// Stage 10: the local contact directory (decision 0023). The views re-query after their own
// commands. Stage 17 (decision 0050): `contactsChanged` names what the user set on contacts
// here, in another window or on another device (names, notes, pictures, merges, spam marks).

/** Contacts per page of the directory (`listContacts` takes 1..200). */
export const CONTACT_PAGE_SIZE = 50;
/** Wait after the last key before searching the directory. */
export const CONTACT_SEARCH_DEBOUNCE_MS = 200;

export const contactDirectoryQueryKey = (query: string, filter: ContactFilter = "people") =>
  ["contactDirectory", query, filter] as const;
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
export function useContactDirectory(query: string, filter: ContactFilter = "people") {
  return useInfiniteQuery({
    queryKey: contactDirectoryQueryKey(query, filter),
    queryFn: ({ pageParam }) => contactsIpc.listContacts(query === "" ? null : query, pageParam, CONTACT_PAGE_SIZE, filter),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.next,
    placeholderData: keepPreviousData,
  });
}

export function useContactDuplicates() {
  return useQuery({ queryKey: contactDuplicatesQueryKey, queryFn: contactsIpc.listContactDuplicates });
}

/**
 * After a change: the directory, the suggestions, the composer's autocomplete and (stage 17)
 * a contact, its mail and the pictures.
 */
export function refreshContacts(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: ["contactDirectory"] });
  void queryClient.invalidateQueries({ queryKey: contactDuplicatesQueryKey });
  void queryClient.invalidateQueries({ queryKey: ["contacts"] });
  void queryClient.invalidateQueries({ queryKey: ["contact"] });
  void queryClient.invalidateQueries({ queryKey: ["contactPictures"] });
}

/** Stage 17: names, notes, pictures, merges or spam marks changed (any window, any device). */
export function handleContactsEvent(queryClient: QueryClient, event: CoreEvent): void {
  if (event.type === "contactsChanged") refreshContacts(queryClient);
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

// ---- Stage 17 (decision 0050): the contacts view ----

/** Recent mail shown with a contact. */
export const CONTACT_MESSAGES_LIMIT = 8;

export const contactQueryKey = (email: string) => ["contact", email] as const;
export const contactMessagesQueryKey = (email: string) => ["contact", email, "messages"] as const;

/** One contact by any of its addresses (the detail pane). */
export function useContact(email: string | null) {
  return useQuery({
    queryKey: contactQueryKey(email ?? ""),
    queryFn: () => contactsIpc.getContact(email ?? ""),
    enabled: email !== null,
  });
}

/** The newest mail from and to a contact. */
export function useContactMessages(email: string | null) {
  return useQuery({
    queryKey: contactMessagesQueryKey(email ?? ""),
    queryFn: () => contactsIpc.listContactMessages(email ?? "", CONTACT_MESSAGES_LIMIT),
    enabled: email !== null,
  });
}

export function useSetContactNote() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ email, note }: { email: string; note: string | null }) => contactsIpc.setContactNote(email, note),
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}

export function useSetContactPicture() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ email, token, crop }: { email: string; token: string; crop: { x: number; y: number; size: number } }) =>
      contactsIpc.setContactPicture(email, token, crop),
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}

export function useRemoveContactPicture() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (email: string) => contactsIpc.removeContactPicture(email),
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}
