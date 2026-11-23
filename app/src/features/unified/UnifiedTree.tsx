import { useQueries } from "@tanstack/react-query";
import { Inbox, Layers } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { treeItemAria, useTreeNavigation } from "../../components/useTreeNavigation";
import { ipc, type Account, type AccountId, type UnifiedView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { sidebarIconClass, sidebarRowClass } from "../layout/sidebarStyles";
import { mailboxesQueryKey } from "../mailboxes/queries";
import { UnreadCount } from "../mailboxes/MailboxTree";
import { useAccountGroups, useHiddenAccounts } from "./queries";

function same(a: UnifiedView | null, b: UnifiedView): boolean {
  if (!a || a.kind !== b.kind) return false;
  return a.kind === "all" || (b.kind === "group" && a.groupId === b.groupId);
}

/** Unread messages in the inboxes of these accounts (the same queries as their folder trees). */
function useInboxUnread(accountIds: readonly AccountId[]): Map<AccountId, number> {
  const results = useQueries({
    queries: accountIds.map((accountId) => ({
      queryKey: mailboxesQueryKey(accountId),
      queryFn: () => ipc.listMailboxes(accountId),
    })),
  });
  return new Map(
    accountIds.map((accountId, index) => [accountId, results[index]?.data?.find((mailbox) => mailbox.role === "inbox")?.unread ?? 0]),
  );
}

/**
 * Top of the accounts-and-folders column (stage 7, docs/DESIGN.md "Seitenspalte"): the unified
 * inbox with the unread messages of every account it shows, and one entry per account group
 * below it. Shown with more than one account or with groups.
 */
export function UnifiedTree({ accounts }: { accounts: readonly Account[] }) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const unified = useUiStore((state) => state.unifiedInbox);
  const showUnifiedInbox = useUiStore((state) => state.showUnifiedInbox);
  const groups = useAccountGroups().data ?? [];
  const hidden = new Set(useHiddenAccounts().data ?? []);
  const unread = useInboxUnread(accounts.map((account) => account.id));
  const headingId = useId();
  const inInbox = view.kind === "mail" && view.mailView === "inbox";
  const total = (ids: readonly AccountId[]) => ids.reduce((sum, id) => sum + (hidden.has(id) ? 0 : (unread.get(id) ?? 0)), 0);
  const entries: { view: UnifiedView; name: string; key: string; unread: number }[] = [
    { view: { kind: "all" }, name: t("unified.title"), key: "all", unread: total(accounts.map((account) => account.id)) },
    ...groups.map((group) => ({ view: { kind: "group", groupId: group.id } as const, name: group.name, key: group.id, unread: total(group.accountIds) })),
  ];
  const items = entries.map((entry, index) => ({ key: entry.key, level: 1, posinset: index + 1, setsize: entries.length, text: entry.name }));
  const nav = useTreeNavigation({
    items,
    currentKey: (inInbox && entries.find((entry) => same(unified, entry.view))?.key) || null,
    onOpen: (key) => {
      const entry = entries.find((candidate) => candidate.key === key);
      if (entry) showUnifiedInbox(entry.view);
    },
  });
  if (accounts.length < 2 && groups.length === 0) return null;
  return (
    <section aria-labelledby={headingId} className="flex flex-col" data-testid="unified-tree">
      <h2 id={headingId} className="sr-only">
        {t("unified.sidebar")}
      </h2>
      <ul {...nav.treeProps} aria-labelledby={headingId} className="flex flex-col">
        {entries.map((entry, index) => {
          const item = items[index];
          if (!item) return null;
          const current = inInbox && same(unified, entry.view);
          const Icon = entry.view.kind === "all" ? Inbox : Layers;
          return (
            <li
              key={entry.key}
              {...treeItemAria(item)}
              {...nav.itemProps(entry.key)}
              aria-current={current ? "page" : undefined}
              aria-label={entry.unread > 0 ? t("sidebar.folderUnread", { name: entry.name, count: entry.unread }) : entry.name}
              onClick={() => {
                showUnifiedInbox(entry.view);
              }}
              className={sidebarRowClass(current, cx("group/folder gap-2 ps-3 pe-2", index === 0 && "font-medium text-primary"))}
              data-testid={`unified-${entry.key}`}
            >
              <Icon aria-hidden className={sidebarIconClass(current)} strokeWidth={1.75} />
              <span className="truncate">{entry.name}</span>
              <UnreadCount count={entry.unread} emphasis={index === 0} />
            </li>
          );
        })}
      </ul>
    </section>
  );
}
