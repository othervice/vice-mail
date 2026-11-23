import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { settingsSyncIpc, type AccountId, type SettingsSyncStatus } from "../../services/ipc";
import { invalidateAccountData } from "../accounts/queries";

export const settingsSyncQueryKey = ["settingsSync", "status"] as const;

/** State, devices and open questions of the settings sync (kept current by `settingsSyncChanged`). */
export function useSettingsSyncStatus() {
  return useQuery({ queryKey: settingsSyncQueryKey, queryFn: settingsSyncIpc.getStatus });
}

/** `settingsSyncChanged`: the core sends the whole status. */
export function applySettingsSyncStatus(queryClient: QueryClient, status: SettingsSyncStatus): void {
  queryClient.setQueryData(settingsSyncQueryKey, status);
}

/**
 * A mutation whose variables hold a passphrase: dropped from the mutation cache as soon as no
 * component shows it (`gcTime: 0`), instead of after the default five minutes.
 */
const FORGET_SECRET = { gcTime: 0 } as const;

interface StatusMutationOptions {
  after?: (queryClient: QueryClient) => Promise<void>;
  secret?: boolean;
}

/** Most commands answer the new status; it replaces the cached one. */
function useStatusMutation<T>(run: (value: T) => Promise<SettingsSyncStatus>, options: StatusMutationOptions = {}) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSuccess: async (status) => {
      applySettingsSyncStatus(queryClient, status);
      await options.after?.(queryClient);
    },
    ...(options.secret && FORGET_SECRET),
  });
}

export interface PassphraseFor {
  accountId: AccountId;
  passphrase: string;
}

export function useEnableSettingsSync() {
  return useStatusMutation(({ accountId, passphrase }: PassphraseFor) => settingsSyncIpc.enable(accountId, passphrase), {
    secret: true,
  });
}

/** Writes nothing: checks the passphrase and shows what joining would take over. */
export function usePreviewSettingsSyncJoin() {
  return useMutation({
    mutationFn: ({ accountId, passphrase }: PassphraseFor) => settingsSyncIpc.previewJoin(accountId, passphrase),
    ...FORGET_SECRET,
  });
}

/** Joining brings accounts, identities and more; the core announces them too, this does not wait. */
export function useJoinSettingsSync() {
  return useStatusMutation(() => settingsSyncIpc.join(), { after: invalidateAccountData });
}

export function useSyncSettingsNow() {
  return useStatusMutation(() => settingsSyncIpc.syncNow());
}

/** "Remove here too" removes the account like the user would; the account list follows. */
export function useResolveSyncQuestion() {
  return useStatusMutation(
    ({ accountId, remove }: { accountId: AccountId; remove: boolean }) => settingsSyncIpc.resolveQuestion(accountId, remove),
    { after: invalidateAccountData },
  );
}

/**
 * Decision 0040: take or keep the servers another device set for an account. Taking them changes
 * the account (servers, writing off, password needed); the account list follows.
 */
export function useResolveServerChange() {
  return useStatusMutation(
    ({ accountId, apply }: { accountId: AccountId; apply: boolean }) => settingsSyncIpc.resolveServerChange(accountId, apply),
    { after: invalidateAccountData },
  );
}

export function useChangeSettingsSyncPassphrase() {
  return useMutation({ mutationFn: (passphrase: string) => settingsSyncIpc.changePassphrase(passphrase), ...FORGET_SECRET });
}

export function useResetSettingsSync() {
  return useStatusMutation((passphrase: string) => settingsSyncIpc.reset(passphrase), { secret: true });
}

export function useDisableSettingsSync() {
  return useStatusMutation(() => settingsSyncIpc.disable());
}

export function useSetSyncDeviceName() {
  return useStatusMutation((name: string) => settingsSyncIpc.setDeviceName(name));
}
