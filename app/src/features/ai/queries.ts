import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  aiIpc,
  type AccountId,
  type AiFunction,
  type AiProfileInput,
  type AiSettingsInfo,
  type AiTransferInput,
  type CoreEvent,
  type MessageId,
  type OpenReplyHint,
  type OpenReplySettings,
} from "../../services/ipc";
import { useAiRequestStore } from "../../stores/aiRequests";

export const aiSettingsQueryKey = ["ai", "settings"] as const;
export const aiAvailabilityQueryKey = ["ai", "availability"] as const;
export const aiRequestLogQueryKey = ["ai", "requests"] as const;
export const openReplyHintsQueryKey = ["ai", "openReplies"] as const;
export const aiModelsQueryKey = (profileId: string) => ["ai", "models", profileId] as const;

/** Rows per page of the request log. */
export const AI_REQUEST_PAGE_SIZE = 100;

/** The page "AI in the app": switch, profiles, functions, cloud permission, open replies. */
export function useAiSettings(enabled = true) {
  return useQuery({ queryKey: aiSettingsQueryKey, queryFn: aiIpc.getSettings, enabled });
}

/**
 * Decision 0028: what a function would send for this input, from the core's dry run (no request
 * to the provider). Asked anew each time the consent step shows; never cached.
 */
export function useAiTransferPreview(input: AiTransferInput | null) {
  return useQuery({
    queryKey: ["ai", "dryRun", input],
    queryFn: () => aiIpc.prepareTransfer(input as AiTransferInput),
    enabled: input !== null,
    gcTime: 0,
    staleTime: 0,
    retry: false,
  });
}

/** Which functions are usable now. Everything AI in the mail views hangs on this. */
export function useAiAvailability() {
  return useQuery({ queryKey: aiAvailabilityQueryKey, queryFn: aiIpc.getAvailability });
}

/** Every settings command answers the whole settings; availability may change with them. */
function useSettingsMutation<T>(run: (value: T) => Promise<AiSettingsInfo>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSuccess: async (settings) => {
      queryClient.setQueryData(aiSettingsQueryKey, settings);
      await queryClient.invalidateQueries({ queryKey: aiAvailabilityQueryKey });
    },
  });
}

export function useSetAiEnabled() {
  return useSettingsMutation((enabled: boolean) => aiIpc.setEnabled(enabled));
}

/** The key in `profile.key` lives only in this call; the answer says `hasKey` and nothing more. */
export function useSaveAiProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (profile: AiProfileInput) => aiIpc.saveProfile(profile),
    onSuccess: async (settings) => {
      queryClient.setQueryData(aiSettingsQueryKey, settings);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: aiAvailabilityQueryKey }),
        queryClient.invalidateQueries({ queryKey: ["ai", "models"] }),
      ]);
    },
    // The variables hold the key: drop the mutation from the cache as soon as it is done.
    gcTime: 0,
  });
}

export function useDeleteAiProfile() {
  return useSettingsMutation((profileId: string) => aiIpc.deleteProfile(profileId));
}

export function useSetAiFunction() {
  return useSettingsMutation(({ aiFunction, profileId }: { aiFunction: AiFunction; profileId: string | null }) =>
    aiIpc.setFunction(aiFunction, profileId),
  );
}

export function useSetAiAccountCloud() {
  return useSettingsMutation(({ accountId, allowed }: { accountId: AccountId; allowed: boolean }) =>
    aiIpc.setAccountCloud(accountId, allowed),
  );
}

export function useSetAiConsent() {
  return useSettingsMutation(({ profileId, granted }: { profileId: string; granted: boolean }) =>
    aiIpc.setConsent(profileId, granted),
  );
}

export function useSetOpenReplySettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: OpenReplySettings) => aiIpc.setOpenReplySettings(settings),
    onSuccess: async (settings) => {
      queryClient.setQueryData(aiSettingsQueryKey, settings);
      await queryClient.invalidateQueries({ queryKey: openReplyHintsQueryKey });
    },
  });
}

/** The model list of a profile; only asked for when the user opens the picker (AI must be on). */
export function useAiModels(profileId: string, enabled: boolean) {
  return useQuery({
    queryKey: aiModelsQueryKey(profileId),
    queryFn: () => aiIpc.listModels(profileId),
    enabled,
    retry: false,
    staleTime: 60_000,
  });
}

export function useTestAiConnection() {
  return useMutation({ mutationFn: (profileId: string) => aiIpc.testConnection(profileId) });
}

/** The request log, newest first, page by page. Reloads when a request finished. */
export function useAiRequestLog() {
  return useInfiniteQuery({
    queryKey: aiRequestLogQueryKey,
    queryFn: ({ pageParam }) => aiIpc.listRequests(pageParam, AI_REQUEST_PAGE_SIZE),
    initialPageParam: null as number | null,
    getNextPageParam: (page) => page.nextBefore,
  });
}

/** Open-reply hints (no AI): there is no event for them, so they reload with the mail. */
export function useOpenReplyHints(enabled = true) {
  return useQuery({ queryKey: openReplyHintsQueryKey, queryFn: aiIpc.listOpenReplyHints, enabled });
}

export function useDismissOpenReplyHint() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (messageId: MessageId) => aiIpc.dismissOpenReplyHint(messageId),
    onMutate: (messageId) => {
      // Gone at once (docs/DESIGN.md: every action shows immediately); a failure brings it back.
      const before = queryClient.getQueryData<OpenReplyHint[]>(openReplyHintsQueryKey);
      queryClient.setQueryData<OpenReplyHint[]>(openReplyHintsQueryKey, (hints) =>
        hints?.filter((hint) => hint.messageId !== messageId),
      );
      return { before };
    },
    onError: (_error, _messageId, context) => {
      if (context?.before) queryClient.setQueryData(openReplyHintsQueryKey, context.before);
    },
  });
}

/**
 * Stage 13 events: `aiRequest` feeds the progress store (what is sent, to whom) and, once a
 * request ended, reloads the request log. `aiSettingsChanged` (decision 0027) carries what is
 * usable now and reloads the settings, so every window (and the panel) follows a change made
 * anywhere. Mail changes may answer or create open-reply hints; the settings list the accounts.
 */
export function handleAiEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "aiRequest":
      useAiRequestStore.getState().apply(event.requestId, event.phase);
      if (event.phase.phase === "finished") void queryClient.invalidateQueries({ queryKey: aiRequestLogQueryKey });
      break;
    case "aiSettingsChanged":
      queryClient.setQueryData(aiAvailabilityQueryKey, event.availability);
      void queryClient.invalidateQueries({ queryKey: aiSettingsQueryKey });
      void queryClient.invalidateQueries({ queryKey: openReplyHintsQueryKey });
      break;
    case "messagesChanged":
      void queryClient.invalidateQueries({ queryKey: openReplyHintsQueryKey });
      break;
    case "accountsChanged":
      void queryClient.invalidateQueries({ queryKey: openReplyHintsQueryKey });
      void queryClient.invalidateQueries({ queryKey: aiSettingsQueryKey });
      break;
    default:
      break;
  }
}
