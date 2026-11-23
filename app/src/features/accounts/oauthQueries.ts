import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { oauthIpc, type AccountId, type AccountSettings, type OAuthPageTexts, type OAuthProvider } from "../../services/ipc";
import { accountsQueryKey, invalidateAccountData } from "./queries";

/** Which sign-ins this build offers; fixed for the life of the app. */
export function useOAuthAvailability() {
  return useQuery({ queryKey: ["oauthAvailability"], queryFn: oauthIpc.availability, staleTime: Infinity });
}

export interface OAuthAccountRequest {
  settings: AccountSettings;
  provider: OAuthProvider;
  texts: OAuthPageTexts;
}

/** Browser step and account in one call; pending while the browser is open. */
export function useAddOAuthAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ settings, provider, texts }: OAuthAccountRequest) => oauthIpc.addAccount(settings, provider, texts),
    onSuccess: async () => {
      await invalidateAccountData(queryClient);
    },
  });
}

export function useSignInAgain() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, texts }: { accountId: AccountId; texts: OAuthPageTexts }) =>
      oauthIpc.signInAgain(accountId, texts),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: accountsQueryKey });
    },
  });
}

/** Ends the wait for the browser; the pending mutation fails with `oauth_cancelled`. */
export function useCancelOAuthSignIn() {
  return useMutation({ mutationFn: () => oauthIpc.cancel() });
}
