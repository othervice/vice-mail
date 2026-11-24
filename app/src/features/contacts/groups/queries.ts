import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { contactListIpc, type Address, type ContactListId } from "../../../services/ipc";
import { refreshContacts } from "../queries";

// Stage 19 (decision 0056): contact groups. `contactsChanged` (any window, any device, also a
// vCard import) reloads them through `refreshContacts`.

export const contactListsQueryKey = ["contactLists"] as const;
export const contactListQueryKey = (id: ContactListId) => ["contactList", id] as const;

export function useContactLists() {
  return useQuery({ queryKey: contactListsQueryKey, queryFn: contactListIpc.list });
}

export function useContactList(id: ContactListId | null) {
  return useQuery({
    queryKey: id ? contactListQueryKey(id) : ["contactList", "none"],
    queryFn: () => (id ? contactListIpc.get(id) : Promise.reject(new Error("no group"))),
    enabled: id !== null,
  });
}

/** Groups for the composer's autocomplete (the core searches the words of their names). */
export function useContactListSuggestions(text: string) {
  const query = text.trim();
  return useQuery({
    queryKey: ["contactListSuggestions", query],
    queryFn: () => contactListIpc.suggest(query, 5),
    enabled: query !== "",
    staleTime: 10_000,
  });
}

function useListMutation<A, R>(run: (args: A) => Promise<R>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSuccess: () => {
      refreshContacts(queryClient);
    },
  });
}

export function useCreateContactList() {
  return useListMutation(({ name, members }: { name: string; members: Address[] }) => contactListIpc.create(name, members));
}

export function useRenameContactList() {
  return useListMutation(({ id, name }: { id: ContactListId; name: string }) => contactListIpc.rename(id, name));
}

export function useDeleteContactList() {
  return useListMutation((id: ContactListId) => contactListIpc.remove(id));
}

export function useAddContactListMembers() {
  return useListMutation(({ id, members }: { id: ContactListId; members: Address[] }) => contactListIpc.addMembers(id, members));
}

export function useRemoveContactListMember() {
  return useListMutation(({ id, email }: { id: ContactListId; email: string }) => contactListIpc.removeMember(id, email));
}
