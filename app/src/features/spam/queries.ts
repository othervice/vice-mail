import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  spamIpc,
  type CoreEvent,
  type MessageId,
  type SpamFilterSettings,
  type SpamFilterStatus,
} from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";

/**
 * Stage 22 (decision 0077): the learning spam filter. Its status (setting, what it learned, the
 * learning from existing mail) follows `spamFilterChanged`; its verdict on a message follows
 * `spamFiltered` and the list events.
 */
export const spamFilterQueryKey = ["spam", "status"] as const;
export const spamVerdictQueryKey = (messageId: MessageId) => ["spam", "verdict", messageId] as const;

export function useSpamFilter() {
  return useQuery({ queryKey: spamFilterQueryKey, queryFn: () => spamIpc.getStatus() });
}

function useStatusMutation<T>(run: (input: T) => Promise<SpamFilterStatus>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSuccess: (status) => {
      queryClient.setQueryData(spamFilterQueryKey, status);
    },
  });
}

export function useSetSpamFilter() {
  return useStatusMutation((settings: SpamFilterSettings) => spamIpc.setSettings(settings));
}

export function useLearnSpamFromMail() {
  return useStatusMutation(() => spamIpc.learnFromMail());
}

export function useForgetSpamFilter() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => spamIpc.forget(),
    onSuccess: (status) => {
      queryClient.setQueryData(spamFilterQueryKey, status);
      refreshSpamMarks(queryClient);
    },
  });
}

export function useSpamVerdict(messageId: MessageId) {
  return useQuery({ queryKey: spamVerdictQueryKey(messageId), queryFn: () => spamIpc.getVerdict(messageId) });
}

/** "Spam" or "Not spam" on this device only; the marks and lists follow. */
export function useMarkSpamLocally() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ messageIds, spam }: { messageIds: MessageId[]; spam: boolean }) => spamIpc.markLocally(messageIds, spam),
    onSettled: () => {
      refreshSpamMarks(queryClient);
    },
  });
}

/** The verdicts shown and every list (the rows carry the filter's mark). */
export function refreshSpamMarks(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: ["spam", "verdict"] });
  void queryClient.invalidateQueries({ queryKey: ["messages"] });
}

/** `spamFilterChanged` carries the status; `spamFiltered` says what happened to new mail. */
export function handleSpamEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "spamFilterChanged":
      queryClient.setQueryData(spamFilterQueryKey, event.status);
      break;
    case "spamFiltered":
      refreshSpamMarks(queryClient);
      useNoticeStore.getState().push({
        kind: "comfort",
        notice: {
          type: "spamFiltered",
          accountId: event.accountId,
          moved: event.moved,
          marked: event.marked,
          batch: event.batch,
        },
      });
      break;
    case "inboxChanged":
    case "messagesChanged":
      // Only the verdict on screen asks again.
      void queryClient.invalidateQueries({ queryKey: ["spam", "verdict"] });
      break;
  }
}
