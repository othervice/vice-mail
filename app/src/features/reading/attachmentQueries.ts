import { keepPreviousData, useMutation, useQuery, useQueryClient, type QueryClient, type QueryKey } from "@tanstack/react-query";
import { refreshQuery } from "../../app/refreshQuery";
import {
  attachmentIpc,
  type AccountId,
  type InboxScope,
  type MarkScope,
  type MessageId,
  type SaveAllAttachmentsResult,
  type UnifiedView,
} from "../../services/ipc";
import type { AttachmentOwner } from "./queries";

/**
 * Stage 16 (decision 0036): previews, "Save all", pins on attachments and the count of mail of
 * blocked senders an inbox list leaves out. Pins and the pinned list reload with `inboxChanged`
 * (the core sends it for every pin change, also after a settings sync pass from another device).
 */

const ownerKey = (owner: AttachmentOwner) => ("fileId" in owner ? `eml:${owner.fileId}` : `message:${String(owner.messageId)}`);

/** A preview is a large `data:` URI; nobody shows it once the dialog closed. */
const PREVIEW_GC_MS = 10_000;

/** One page of a preview (0 for images and text). Asked once per opening, never retried. */
export function useAttachmentPreview(owner: AttachmentOwner, index: number, page: number) {
  return useQuery({
    queryKey: ["attachmentPreview", ownerKey(owner), index, page],
    queryFn: () =>
      "fileId" in owner
        ? attachmentIpc.previewEmlAttachment(owner.fileId, index, page)
        : attachmentIpc.previewAttachment(owner.messageId, index, page),
    gcTime: PREVIEW_GC_MS,
    staleTime: Infinity,
    retry: false,
    // The page shown stays until the next one is there.
    placeholderData: keepPreviousData,
  });
}

/** Every attachment into a folder the user picks in the system dialog. */
export function useSaveAllAttachments(owner: AttachmentOwner) {
  return useMutation({
    mutationFn: (): Promise<SaveAllAttachmentsResult> =>
      "fileId" in owner ? attachmentIpc.saveAllEmlAttachments(owner.fileId) : attachmentIpc.saveAllAttachments(owner.messageId),
  });
}

export const attachmentPinsQueryKey = (messageId: MessageId) => ["attachmentPins", messageId] as const;

/** The pinned indexes of a message (only stored messages have pins, never `.eml` files). */
export function useAttachmentPins(messageId: MessageId | null) {
  return useQuery({
    queryKey: attachmentPinsQueryKey(messageId ?? -1),
    queryFn: () => attachmentIpc.getAttachmentPins(messageId ?? -1),
    enabled: messageId !== null,
  });
}

export const pinnedAttachmentsQueryKey = (scope: MarkScope) => ["pinnedAttachments", scope] as const;

/** "Pinned attachments" of a scope (like the reading list: an account, a group or all). */
export function usePinnedAttachments(scope: MarkScope) {
  return useQuery({ queryKey: pinnedAttachmentsQueryKey(scope), queryFn: () => attachmentIpc.listPinnedAttachments(scope) });
}

function refreshAll(queryClient: QueryClient, prefix: QueryKey): void {
  for (const query of queryClient.getQueryCache().findAll({ queryKey: prefix })) refreshQuery(queryClient, query.queryKey);
}

/** After `inboxChanged` or `messagesChanged`: pins and the pinned list (a message may be gone). */
export function refreshAttachmentPins(queryClient: QueryClient): void {
  refreshAll(queryClient, ["attachmentPins"]);
  refreshAll(queryClient, ["pinnedAttachments"]);
}

/** Pins or unpins one attachment of a message; the pins and the pinned list follow at once. */
export function useSetAttachmentPinned() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ messageId, index, pinned }: { messageId: MessageId; index: number; pinned: boolean }) =>
      attachmentIpc.setAttachmentPinned(messageId, index, pinned),
    onSettled: async (_data, _error, { messageId }) => {
      await queryClient.invalidateQueries({ queryKey: attachmentPinsQueryKey(messageId) });
      await queryClient.invalidateQueries({ queryKey: ["pinnedAttachments"] });
    },
  });
}

/**
 * The count of messages of blocked senders an inbox list leaves out. Keyed below the lists it
 * belongs to (`["messages", account, folder, …]`, `["unified", view, …]`), so the events that
 * reload those lists (`messagesChanged`, `inboxChanged`, `mailboxesChanged`) reload it too.
 */
export const BLOCKED_HIDDEN = "blockedHidden";

export function blockedHiddenQueryKey(scope: InboxScope, accountId: AccountId | null): QueryKey {
  return scope.kind === "unified" ? ["unified", scope.view satisfies UnifiedView, BLOCKED_HIDDEN] : ["messages", accountId, scope.mailboxId, BLOCKED_HIDDEN];
}

export function useBlockedHidden(scope: InboxScope, accountId: AccountId | null) {
  return useQuery({ queryKey: blockedHiddenQueryKey(scope, accountId), queryFn: () => attachmentIpc.countBlockedHidden(scope) });
}

/** After blocking or unblocking: every count asks again (hidden mail shows or goes). */
export function refreshBlockedHidden(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({
    predicate: (query) => query.queryKey[query.queryKey.length - 1] === BLOCKED_HIDDEN,
  });
}
