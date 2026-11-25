import { useQueries } from "@tanstack/react-query";
import { Inbox, Layers } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { treeItemAria, useTreeNavigation } from "../../components/useTreeNavigation";
import { ipc, type Account, type AccountId, type Mailbox, type UnifiedView } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { OverflowMenu } from "../actions/OverflowMenu";
import { sidebarIconClass, sidebarRowClass } from "../layout/sidebarStyles";
import { mailboxesQueryKey } from "../mailboxes/queries";
import { UnreadCount } from "../mailboxes/MailboxTree";
import { useMarkReadItem, type MarkReadTarget } from "../mailboxes/markRead";
import { RowMenuSlot } from "../organize/folders";
import { useAccountGroups, useHiddenAccounts } from "./queries";

function same(a: UnifiedView | null, b: UnifiedView): boolean {
  if (!a || a.kind !== b.kind) return false;
  return a.kind === "all" || (b.kind === "group" && a.groupId === b.groupId);
}

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
function UnifiedRowMenu({ name, targets, returnFocus }: { name: string; targets: readonly MarkReadTarget[]; returnFocus: () => void }) {
  const { t } = useTranslation();
  const items = useMarkReadItem("inboxes", targets, name);
  return (
    <OverflowMenu label={t("markRead.menu", { name })} items={items} tabIndex={-1} returnFocus={returnFocus} narrow data-testid="unified-row-menu" />
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
  const inboxes = useInboxes(accounts.map((account) => account.id));
  const headingId = useId();
  const inInbox = view.kind === "mail" && view.mailView === "inbox";
  const total = (ids: readonly AccountId[]) => ids.reduce((sum, id) => sum + (hidden.has(id) ? 0 : (inboxes.get(id)?.unread ?? 0)), 0);
  // The inboxes an entry shows: of its accounts that are not hidden.
  const targets = (ids: readonly AccountId[]): MarkReadTarget[] =>
    accounts.flatMap((account) => {
      const inbox = inboxes.get(account.id);
      return ids.includes(account.id) && !hidden.has(account.id) && inbox ? [{ account, mailboxes: [inbox] }] : [];
    });
  const allIds = accounts.map((account) => account.id);
  const entries: { view: UnifiedView; name: string; key: string; unread: number; targets: MarkReadTarget[] }[] = [
    { view: { kind: "all" }, name: t("unified.title"), key: "all", unread: total(allIds), targets: targets(allIds) },
    ...groups.map((group) => ({
      view: { kind: "group", groupId: group.id } as const,
      name: group.name,
      key: group.id,
      unread: total(group.accountIds),
      targets: targets(group.accountIds),
    })),
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
              <RowMenuSlot>
                <UnifiedRowMenu
                  name={entry.name}
                  targets={entry.targets}
                  returnFocus={() => {
                    nav.focus(entry.key);
                  }}
                />
              </RowMenuSlot>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
