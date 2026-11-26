import { useQueries } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ipc, type Account, type AccountId, type Mailbox, type UnifiedView } from "../../services/ipc";
import { OverflowMenu } from "../actions/OverflowMenu";
import { mailboxesQueryKey } from "../mailboxes/queries";
import { useMarkReadItem, type MarkReadTarget } from "../mailboxes/markRead";
import { useAccountGroups, useHiddenAccounts } from "./queries";

/** The inboxes of these accounts, with their unread counters (the same queries as their folder trees). */
function useInboxes(accountIds: readonly AccountId[]): Map<AccountId, Mailbox | undefined> {
  const results = useQueries({
    queries: accountIds.map((accountId) => ({
      queryKey: mailboxesQueryKey(accountId),
      queryFn: () => ipc.listMailboxes(accountId),
    })),
  });
  return new Map(accountIds.map((accountId, index) => [accountId, results[index]?.data?.find((mailbox) => mailbox.role === "inbox")]));
}

/**
 * Stage 21 (decision 0063): the "…" of the unified inbox or a group: "Mark all as read", the
 * inboxes of its accounts that are shown. Nothing while they have no unread mail.
 */
export function UnifiedRowMenu({ name, targets, returnFocus }: { name: string; targets: readonly MarkReadTarget[]; returnFocus: () => void }) {
  const { t } = useTranslation();
  const items = useMarkReadItem("inboxes", targets, name);
  return (
    <OverflowMenu label={t("markRead.menu", { name })} items={items} tabIndex={-1} returnFocus={returnFocus} narrow data-testid="unified-row-menu" />
  );
}

/** One entry of the unified inbox or a group (stage 7), as the column lists them. */
export interface UnifiedEntry {
  view: UnifiedView;
  name: string;
  /** `all` or the group's id. */
  key: string;
  /** Unread mail in the inboxes of its accounts that are shown. */
  unread: number;
  /** Those inboxes, for "Mark all as read". */
  targets: MarkReadTarget[];
}

export function sameUnifiedView(a: UnifiedView | null, b: UnifiedView): boolean {
  if (!a || a.kind !== b.kind) return false;
  return a.kind === "all" || (b.kind === "group" && a.groupId === b.groupId);
}

/**
 * Stage 7 (docs/DESIGN.md "Seitenspalte"): the unified inbox with the unread messages of every
 * account it shows, and one entry per account group. Stage 23 (decision 0085): the column shows
 * the first as "Inbox" of "All accounts" and the groups under "More".
 */
export function useUnifiedEntries(accounts: readonly Account[]): { all: UnifiedEntry; groups: UnifiedEntry[] } {
  const { t } = useTranslation();
  const groups = useAccountGroups().data ?? [];
  const hidden = new Set(useHiddenAccounts().data ?? []);
  const inboxes = useInboxes(accounts.map((account) => account.id));
  const total = (ids: readonly AccountId[]) => ids.reduce((sum, id) => sum + (hidden.has(id) ? 0 : (inboxes.get(id)?.unread ?? 0)), 0);
  // The inboxes an entry shows: of its accounts that are not hidden.
  const targets = (ids: readonly AccountId[]): MarkReadTarget[] =>
    accounts.flatMap((account) => {
      const inbox = inboxes.get(account.id);
      return ids.includes(account.id) && !hidden.has(account.id) && inbox ? [{ account, mailboxes: [inbox] }] : [];
    });
  const allIds = accounts.map((account) => account.id);
  return {
    all: { view: { kind: "all" }, name: t("unified.title"), key: "all", unread: total(allIds), targets: targets(allIds) },
    groups: groups.map((group) => ({
      view: { kind: "group", groupId: group.id } as const,
      name: group.name,
      key: group.id,
      unread: total(group.accountIds),
      targets: targets(group.accountIds),
    })),
  };
}
