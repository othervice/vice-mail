import { keepPreviousData, useMutation, useQuery, useQueryClient, type QueryClient, type QueryKey } from "@tanstack/react-query";
import {
  emlIpc,
  ipc,
  type AccountId,
  type BodyFormat,
  type EmlFileId,
  type IpcError,
  type MessageId,
  type MessageView,
  type PrefetchSettings,
  type PrefetchStatus,
  type RemoteContentScope,
  type RenderToken,
} from "../../services/ipc";
import { useMailtoStore } from "../../stores/mailto";

/**
 * A view nobody shows any more is dropped after this time. The core keeps the link targets of
 * about 32 renderings; a view that waited longer in the cache would point at forgotten ones.
 */
const MESSAGE_VIEW_GC_MS = 30_000;

/** Keyed by account first, like the lists, so events of one account can address its views. */
export const messageViewsQueryKey = (accountId: AccountId, messageId: MessageId) =>
  ["message", accountId, messageId] as const;

export const messageViewQueryKey = (accountId: AccountId, messageId: MessageId, format: BodyFormat) =>
  [...messageViewsQueryKey(accountId, messageId), format] as const;

/**
 * What the reading pane shows for one message in one format. The core answers at once: with a
 * rendering, with `loading` (a fetch started; `messageBodyAvailable` follows) or `unavailable`.
 * Switching the format keeps the previous rendering on screen until the new one is there.
 */
export function useMessageView(accountId: AccountId, messageId: MessageId, format: BodyFormat) {
  return useQuery({
    queryKey: messageViewQueryKey(accountId, messageId, format),
    queryFn: () => ipc.getMessage(messageId, format),
    gcTime: MESSAGE_VIEW_GC_MS,
    placeholderData: keepPreviousData,
  });
}

/** The cached views of one message (one per format asked for). */
function viewQueries(queryClient: QueryClient, accountId: AccountId, messageId: MessageId) {
  return queryClient.getQueryCache().findAll({ queryKey: messageViewsQueryKey(accountId, messageId) });
}

function waitsForBody(view: MessageView | undefined): boolean {
  return view === undefined || view.body.kind === "loading" || view.body.kind === "unavailable";
}

/**
 * `messageBodyAvailable`: ask again for the views of that message that wait for their body.
 * Views that already show it stay as they are; asking again would render anew and reload the
 * frame (new render token), losing the reader's place. A view still on its way is asked again only
 * if it arrives without the body (`refresh` gets `waitsForBody` for that).
 */
export function messageBodyAvailable(
  queryClient: QueryClient,
  accountId: AccountId,
  messageId: MessageId,
  refresh: (queryKey: QueryKey, stillWaits: (view: MessageView | undefined) => boolean) => void,
): void {
  for (const query of viewQueries(queryClient, accountId, messageId)) {
    if (waitsForBody(query.state.data as MessageView | undefined)) refresh(query.queryKey, waitsForBody);
  }
}

/**
 * `messageBodyFailed`: the views waiting for that body show the failure. No new request: asking
 * again would only start the next fetch. "Try again" does that, on the user's click.
 */
export function messageBodyFailed(queryClient: QueryClient, accountId: AccountId, messageId: MessageId, error: IpcError): void {
  for (const query of viewQueries(queryClient, accountId, messageId)) {
    queryClient.setQueryData<MessageView>(query.queryKey, (view) =>
      view?.body.kind === "loading" ? { ...view, body: { kind: "unavailable", reason: "fetchFailed", error } } : view,
    );
  }
}

export const remoteSendersQueryKey = (accountId: AccountId) => ["remoteSenders", accountId] as const;

/**
 * Loads the blocked external content of the shown rendering through the core. The answer is a
 * new rendering of the same message, which replaces the shown one. `sender` also stores the
 * sender: other cached views of the account are then out of date and reload when shown next.
 */
export function useLoadRemoteContent(accountId: AccountId, messageId: MessageId, format: BodyFormat) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (scope: RemoteContentScope) => ipc.loadRemoteContent(messageId, scope),
    onSuccess: async (view, scope) => {
      if (scope === "sender") {
        await queryClient.invalidateQueries({ queryKey: ["message", accountId], refetchType: "none" });
        await queryClient.invalidateQueries({ queryKey: remoteSendersQueryKey(accountId) });
      }
      queryClient.setQueryData(messageViewQueryKey(accountId, messageId, format), view);
    },
  });
}

/** Senders of an account whose external content loads without asking (Settings, Reading). */
export function useRemoteSenders(accountId: AccountId) {
  return useQuery({ queryKey: remoteSendersQueryKey(accountId), queryFn: () => ipc.listRemoteContentSenders(accountId) });
}

export function useRevokeRemoteContent(accountId: AccountId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sender: string) => ipc.revokeRemoteContent(accountId, sender),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["message", accountId], refetchType: "none" });
      await queryClient.invalidateQueries({ queryKey: remoteSendersQueryKey(accountId) });
    },
  });
}

/** The real target of a clicked link, checked by the core. Never cached: one answer per click. */
export function useDescribeLink(renderToken: RenderToken, index: number) {
  return useQuery({
    queryKey: ["link", renderToken, index],
    queryFn: () => ipc.describeLink(renderToken, index),
    gcTime: 0,
  });
}

/**
 * Opens a confirmed link. A `mailto:` link comes back as a compose request and goes to the same
 * queue as links from the system, so it never replaces a message being written.
 */
export function useOpenLink() {
  return useMutation({
    mutationFn: ({ renderToken, index }: { renderToken: RenderToken; index: number }) => ipc.openLink(renderToken, index),
    onSuccess: (opened) => {
      if (opened.kind === "compose") useMailtoStore.getState().enqueue([opened.request]);
    },
  });
}

/** Whose attachments: a stored message, or (stage 9, decision 0027) an opened `.eml` file. */
export type AttachmentOwner = { messageId: MessageId } | { fileId: EmlFileId };

export function useSaveAttachment(owner: AttachmentOwner, index: number) {
  return useMutation({
    mutationFn: () => ("fileId" in owner ? emlIpc.saveAttachment(owner.fileId, index) : ipc.saveAttachment(owner.messageId, index)),
  });
}

export function useOpenAttachment(owner: AttachmentOwner, index: number) {
  return useMutation({
    mutationFn: (confirmedExecutable: boolean) =>
      "fileId" in owner
        ? emlIpc.openAttachment(owner.fileId, index, confirmedExecutable)
        : ipc.openAttachment(owner.messageId, index, confirmedExecutable),
  });
}

export const prefetchSettingsQueryKey = ["prefetchSettings"] as const;
export const prefetchStatusQueryKey = (accountId: AccountId) => ["prefetchStatus", accountId] as const;

export function usePrefetchSettings() {
  return useQuery({ queryKey: prefetchSettingsQueryKey, queryFn: ipc.getPrefetchSettings });
}

/** Saves at once in the UI and rolls back if the core refuses (like the preferences). */
export function useSetPrefetchSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ipc.setPrefetchSettings,
    onMutate: async (next: PrefetchSettings) => {
      await queryClient.cancelQueries({ queryKey: prefetchSettingsQueryKey });
      const previous = queryClient.getQueryData<PrefetchSettings>(prefetchSettingsQueryKey);
      queryClient.setQueryData(prefetchSettingsQueryKey, next);
      return { previous };
    },
    onError: (_error, _next, context) => {
      if (context?.previous) queryClient.setQueryData(prefetchSettingsQueryKey, context.previous);
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(prefetchSettingsQueryKey, saved);
    },
  });
}

/** Where the prefetch of an account stands; `prefetchProgress` keeps it current. */
export function usePrefetchStatus(accountId: AccountId) {
  return useQuery({ queryKey: prefetchStatusQueryKey(accountId), queryFn: () => ipc.getPrefetchStatus(accountId) });
}

export function applyPrefetchStatus(queryClient: QueryClient, accountId: AccountId, status: PrefetchStatus): void {
  queryClient.setQueryData(prefetchStatusQueryKey(accountId), status);
}
