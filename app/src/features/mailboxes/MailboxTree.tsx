import { ChevronDown, ChevronRight, Folder, type LucideIcon } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, MailboxId } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { useMailboxes } from "./queries";
import { buildMailboxTree, isRoleView, type MailboxNode } from "./tree";
import { useDropTarget } from "./useDropTarget";

interface MailboxTreeProps {
  account: Account;
  /** The folder the mail view shows right now, marked as current. */
  currentMailboxId: MailboxId | null;
}

/** Visible counter; screen readers get it through the button's name ("Inbox, 3 unread"). */
function UnreadCount({ count }: { count: number }) {
  const { i18n } = useTranslation();
  if (count <= 0) return null;
  return (
    <span aria-hidden className="ms-auto shrink-0 text-12 font-medium text-secondary tabular-nums">
      {new Intl.NumberFormat(i18n.language).format(count)}
    </span>
  );
}

function FolderIcon({ icon: Icon }: { icon: LucideIcon }) {
  return <Icon aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />;
}

interface ItemProps {
  node: MailboxNode;
  account: Account;
  currentMailboxId: MailboxId | null;
}

function MailboxItem({ node, account, currentMailboxId }: ItemProps) {
  const { t } = useTranslation();
  const collapsed = useUiStore((state) => state.collapsedMailboxes.has(node.mailbox.id));
  const toggleMailbox = useUiStore((state) => state.toggleMailbox);
  const showMailView = useUiStore((state) => state.showMailView);
  const showMailbox = useUiStore((state) => state.showMailbox);
  const selectAccount = useUiStore((state) => state.selectAccount);
  const { mailbox, children } = node;
  const name = mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`);
  const current = mailbox.id === currentMailboxId;
  // Rows dragged out of the list can be dropped here (a move of the dragged messages).
  const drop = useDropTarget(mailbox);

  const open = () => {
    // Folders with a view in the rail open that view, so rail and tree agree on what is shown.
    if (isRoleView(mailbox.role)) {
      selectAccount(account.id);
      showMailView(mailbox.role);
    } else {
      showMailbox(account.id, mailbox.id);
    }
  };

  const label = (
    <>
      <FolderIcon icon={isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder} />
      <span className="truncate">{name}</span>
      <UnreadCount count={mailbox.unread} />
    </>
  );

  return (
    <li>
      <div className="flex items-center gap-1">
        {children.length > 0 ? (
          <button
            type="button"
            aria-expanded={!collapsed}
            aria-label={collapsed ? t("sidebar.expand", { name }) : t("sidebar.collapse", { name })}
            onClick={() => {
              toggleMailbox(mailbox.id);
            }}
            className="flex size-6 shrink-0 items-center justify-center rounded-control text-tertiary hover:bg-hover hover:text-primary"
          >
            {collapsed ? (
              <ChevronRight aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            ) : (
              <ChevronDown aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            )}
          </button>
        ) : (
          <span aria-hidden className="size-6 shrink-0" />
        )}
        {mailbox.selectable ? (
          <button
            type="button"
            aria-current={current ? "page" : undefined}
            aria-label={mailbox.unread > 0 ? t("sidebar.folderUnread", { name, count: mailbox.unread }) : name}
            title={mailbox.path}
            onClick={open}
            {...drop.handlers}
            data-testid={`mailbox-${String(mailbox.id)}`}
            data-drop-target={drop.over ? "over" : drop.accepts ? "ready" : undefined}
            className={cx(
              "flex h-8 min-w-0 flex-1 items-center gap-2 rounded-control px-2 text-13 transition-colors",
              current ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-hover hover:text-primary",
              drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
            )}
          >
            {label}
          </button>
        ) : (
          <span title={mailbox.path} className="flex h-8 min-w-0 flex-1 items-center gap-2 px-2 text-13 text-tertiary">
            {label}
          </span>
        )}
      </div>
      {children.length > 0 && !collapsed && (
        <ul className="ps-4">
          {children.map((child) => (
            <MailboxItem key={child.mailbox.id} node={child} account={account} currentMailboxId={currentMailboxId} />
          ))}
        </ul>
      )}
    </li>
  );
}

/**
 * The folders of one account: role folders first in a fixed order, then the account's own
 * folders as a tree, each with its unread counter. Kept current by `mailboxesChanged`.
 */
export function MailboxTree({ account, currentMailboxId }: MailboxTreeProps) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id);
  const tree = useMemo(() => buildMailboxTree(mailboxes.data ?? []), [mailboxes.data]);

  if (mailboxes.isPending) return null;
  if (mailboxes.isError) {
    return (
      <ErrorNotice
        context={t("sidebar.loadFailed")}
        error={mailboxes.error}
        onRetry={() => {
          void mailboxes.refetch();
        }}
      />
    );
  }
  if (tree.length === 0) return <p className="px-2 text-12 text-tertiary">{t("sidebar.noFolders")}</p>;
  return (
    <ul aria-label={t("sidebar.folders", { account: account.displayName || account.email })} className="flex flex-col">
      {tree.map((node) => (
        <MailboxItem key={node.mailbox.id} node={node} account={account} currentMailboxId={currentMailboxId} />
      ))}
    </ul>
  );
}
