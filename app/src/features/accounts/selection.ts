import type { Account, AccountStatus } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useAccounts } from "./queries";

/**
 * The account the rail's views show. Until the unified inbox (stage 7) every view belongs to one
 * account: the one picked in the account switcher, otherwise the oldest.
 */
export function useSelectedAccount() {
  const accounts = useAccounts();
  const selectedId = useUiStore((state) => state.selectedAccountId);
  const list = accounts.data ?? [];
  const account = list.find((a) => a.id === selectedId) ?? list[0] ?? null;
  return { accounts, account };
}

/** True when there are accounts and none of them is connected: the app as a whole is offline. */
export function allOffline(accounts: readonly Account[]): boolean {
  return accounts.length > 0 && accounts.every((a) => a.status.state === "offline");
}

/** The account has never finished a sync pass and is working on its first one. */
export function isFirstSyncRunning(status: AccountStatus): boolean {
  return status.lastSyncAt === null && (status.state === "connecting" || status.state === "syncing");
}
