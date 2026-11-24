import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { refreshQuery } from "../../app/refreshQuery";
import { blockedSendersQueryKey } from "../blocked/queries";
import { refreshAttachmentPins } from "../reading/attachmentQueries";
import {
  comfortIpc,
  type AccountId,
  type InboxSettings,
  type MailboxId,
  type MessageCategory,
  type MessageId,
  type NotificationSettings,
  type SenderKey,
} from "../../services/ipc";

/**
 * Stage 9 query keys. The lists of the inbox and of Later, Pinned and Tasks sit below the folder
 * key `["messages", accountId, mailboxId]`, so everything that reloads a folder's lists (the
 * action controller, `mailboxesChanged`) reloads them too.
 */
export type MarkListKind = "later" | "pinned" | "tasks" | "readingList" | "flagged";

export const inboxQueryKey = (accountId: AccountId, mailboxId: MailboxId) =>
  ["messages", accountId, mailboxId, "inbox"] as const;
export const priorityQueryKey = (accountId: AccountId, mailboxId: MailboxId) =>
  ["messages", accountId, mailboxId, "priority"] as const;
export const markListQueryKey = (accountId: AccountId, mailboxId: MailboxId, kind: MarkListKind, filter: string) =>
  ["messages", accountId, mailboxId, kind, filter] as const;

export const inboxSettingsQueryKey = ["comfort", "inboxSettings"] as const;
export const senderRulesQueryKey = ["comfort", "senderRules"] as const;
export const messageCategoryQueryKey = (messageId: MessageId) => ["comfort", "category", messageId] as const;
export const snoozeTimesQueryKey = ["comfort", "snoozeTimes"] as const;
export const notificationSettingsQueryKey = ["comfort", "notificationSettings"] as const;

// Stage 16: the reading list, and the "Unread only" and "Unread first" lists of a folder; the
// count of mail of blocked senders an inbox leaves out (decision 0036).
const COMFORT_LIST_KINDS = new Set([
  "inbox",
  "priority",
  "later",
  "pinned",
  "tasks",
  "readingList",
  "flagged",
  "unread",
  "unreadFirst",
  "blockedHidden",
]);

/**
 * Reloads the stage 9 lists after `inboxChanged` (pins, snoozes, tasks or categories changed, a
 * snooze came back) or `messagesChanged`: of one account, or of all with `null`. Only lists on
 * screen ask the core again; `refreshQuery` merges bursts.
 */
export function refreshComfortLists(queryClient: QueryClient, accountId: AccountId | null): void {
  const queries = queryClient.getQueryCache().findAll({
    queryKey: accountId === null ? ["messages"] : ["messages", accountId],
    predicate: (query) => COMFORT_LIST_KINDS.has(String(query.queryKey[3])),
  });
  for (const query of queries) refreshQuery(queryClient, query.queryKey);
}

const MARK_KINDS = new Set(["later", "pinned", "tasks", "readingList", "flagged"]);

/**
 * After `messagesChanged` of one folder: its sorted inbox, and the mark views of the account
 * (they gather messages of every folder of it).
 */
export function refreshComfortListsOf(queryClient: QueryClient, accountId: AccountId, mailboxId: MailboxId): void {
  const queries = queryClient.getQueryCache().findAll({
    queryKey: ["messages", accountId],
    predicate: (query) => {
      const kind = String(query.queryKey[3]);
      return MARK_KINDS.has(kind) || (query.queryKey[2] === mailboxId && COMFORT_LIST_KINDS.has(kind));
    },
  });
  for (const query of queries) refreshQuery(queryClient, query.queryKey);
}

/** After `inboxChanged`: the lists, and what "Why here?" and the sender rules show. */
export function handleInboxChanged(queryClient: QueryClient, accountId: AccountId | null): void {
  refreshComfortLists(queryClient, accountId);
  void queryClient.invalidateQueries({ queryKey: ["comfort", "category"] });
  refreshQuery(queryClient, senderRulesQueryKey);
  // Stage 16: a sender blocked or unblocked here or on another device (settings sync).
  refreshQuery(queryClient, blockedSendersQueryKey);
  // Stage 16 (decision 0036): pins on attachments.
  refreshAttachmentPins(queryClient);
}

export function useInboxSettings() {
  return useQuery({ queryKey: inboxSettingsQueryKey, queryFn: () => comfortIpc.getInboxSettings() });
}

/** Categories and bundles on or off (device setting). `inboxChanged` reloads the lists. */
export function useSetInboxSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: InboxSettings) => comfortIpc.setInboxSettings(settings),
    onSuccess: (settings) => {
      queryClient.setQueryData(inboxSettingsQueryKey, settings);
    },
  });
}

export function useMessageCategory(messageId: MessageId) {
  return useQuery({
    queryKey: messageCategoryQueryKey(messageId),
    queryFn: () => comfortIpc.getMessageCategory(messageId),
  });
}

export function useSenderRules() {
  return useQuery({ queryKey: senderRulesQueryKey, queryFn: () => comfortIpc.listSenderRules() });
}

/** "Always as …" (or `null`: back to automatic) for a sender, domain or list. */
export function useSetSenderCategory() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ key, category }: { key: SenderKey; category: MessageCategory | null }) =>
      comfortIpc.setSenderCategory(key, category),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: senderRulesQueryKey });
      void queryClient.invalidateQueries({ queryKey: ["comfort", "category"] });
    },
  });
}

export function useSetSenderImportant() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ key, important }: { key: SenderKey; important: boolean }) =>
      comfortIpc.setSenderImportant(key, important),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: senderRulesQueryKey });
      void queryClient.invalidateQueries({ queryKey: ["comfort", "category"] });
    },
  });
}

export function useDeleteSenderRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => comfortIpc.deleteSenderRule(id),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: senderRulesQueryKey });
    },
  });
}

/**
 * The times of the snooze menu. Asked anew each time the menu opens ("later today" disappears in
 * the evening, "tomorrow" moves at midnight), never cached across openings.
 */
export function useSnoozeTimes() {
  return useQuery({
    queryKey: snoozeTimesQueryKey,
    queryFn: () => comfortIpc.getSnoozeTimes(),
    staleTime: 0,
    gcTime: 0,
  });
}

export function useNotificationSettings() {
  return useQuery({ queryKey: notificationSettingsQueryKey, queryFn: () => comfortIpc.getNotificationSettings() });
}

export function useSetNotificationSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: NotificationSettings) => comfortIpc.setNotificationSettings(settings),
    onSuccess: (settings) => {
      queryClient.setQueryData(notificationSettingsQueryKey, settings);
    },
  });
}
