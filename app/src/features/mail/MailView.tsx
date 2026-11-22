import { FolderX, Hourglass, Inbox, Search, SquarePen } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { ViewHeader } from "../../components/ViewHeader";
import type { Account, Mailbox } from "../../services/ipc";
import { useUiStore, type View } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { AccountErrorBanner, SyncStatus } from "../accounts/AccountStatus";
import { isFirstSyncRunning, useSelectedAccount } from "../accounts/selection";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { useMailboxes } from "../mailboxes/queries";
import { isRoleView, mailboxWithRole } from "../mailboxes/tree";
import { MessageList } from "../messages/MessageList";

export type MailViewState = Extract<View, { kind: "mail" } | { kind: "mailbox" }>;

/** Name of a folder as the UI shows it: the translated role, or the folder's own name. */
export function useMailboxLabel(): (mailbox: Mailbox) => string {
  const { t } = useTranslation();
  return (mailbox) => (mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`));
}

/** The account a view belongs to: its colour next to its name, never the colour alone. */
function AccountLine({ account }: { account: Account }) {
  const { t } = useTranslation();
  return (
    <>
      <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
      <span className="truncate">
        {account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email}
      </span>
    </>
  );
}

interface MailBodyProps {
  view: MailViewState;
  account: Account;
}

/** What a mail view shows for one account: its folder's messages, or why there are none. */
function MailBody({ view, account }: MailBodyProps) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id);
  const label = useMailboxLabel();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const accountName = account.displayName || account.email;

  const roleView = view.kind === "mail" && isRoleView(view.mailView) ? view.mailView : null;
  if (view.kind === "mail" && !roleView) {
    // Views without a function yet (Later, Tasks, Pinned, Saved views) say so honestly.
    const name = t(`nav.${view.mailView}`);
    return <EmptyState icon={Hourglass} title={t("mail.notYet.title")} body={t("mail.notYet.body", { view: name })} />;
  }
  if (mailboxes.isPending) return null;
  if (mailboxes.isError) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("sidebar.loadFailed")}
          error={mailboxes.error}
          onRetry={() => {
            void mailboxes.refetch();
          }}
        />
      </div>
    );
  }

  const mailbox =
    view.kind === "mailbox"
      ? mailboxes.data.find((m) => m.id === view.mailboxId)
      : roleView && mailboxWithRole(mailboxes.data, roleView);

  if (!mailbox) {
    if (mailboxes.data.length === 0 && isFirstSyncRunning(account.status)) {
      return (
        <EmptyState
          icon={Inbox}
          title={t("list.firstSync.title")}
          body={t("list.firstSync.body", { account: accountName })}
        />
      );
    }
    if (!roleView) {
      return <EmptyState icon={FolderX} title={t("mail.folderGone.title")} body={t("mail.folderGone.body")} />;
    }
    const role = t(`roles.${roleView}`);
    return (
      <EmptyState
        icon={FolderX}
        title={t("mail.noRoleFolder.title", { role })}
        body={t("mail.noRoleFolder.body", { role, account: accountName })}
        action={{
          label: t("mail.noRoleFolder.action"),
          onClick: () => {
            showAccountsPage({ kind: "account", accountId: account.id });
          },
        }}
      />
    );
  }

  return (
    <MessageList
      key={mailbox.id}
      account={account}
      mailbox={mailbox}
      folderName={label(mailbox)}
      showRecipients={mailbox.role === "sent" || mailbox.role === "drafts"}
    />
  );
}

interface MailViewProps {
  view: MailViewState;
}

/**
 * A mail view: header with the folder and its account, then the message list. Until the
 * unified inbox (stage 7) every view shows the folder of the selected account.
 */
export function MailView({ view }: MailViewProps) {
  const { t } = useTranslation();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const dragRegion = useWindowDragRegion();
  const { accounts, account } = useSelectedAccount();
  const mailboxes = useMailboxes(view.kind === "mailbox" ? view.accountId : null);
  const label = useMailboxLabel();

  const folder = view.kind === "mailbox" ? mailboxes.data?.find((m) => m.id === view.mailboxId) : undefined;
  const title = view.kind === "mail" ? t(`nav.${view.mailView}`) : folder ? label(folder) : t("nav.folders");

  let body;
  if (accounts.isPending) {
    body = null;
  } else if (accounts.isError) {
    body = (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("settings.accounts.loadFailed")}
          error={accounts.error}
          onRetry={() => {
            void accounts.refetch();
          }}
        />
      </div>
    );
  } else if (!account) {
    body = (
      <EmptyState
        icon={Inbox}
        title={t("mail.noAccount.title")}
        body={t("mail.noAccount.body")}
        action={{
          label: t("mail.noAccount.action"),
          onClick: () => {
            showAccountsPage({ kind: "add" });
          },
        }}
      />
    );
  } else {
    body = (
      <>
        <AccountErrorBanner account={account} />
        <MailBody view={view} account={account} />
      </>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewHeader title={title} subtitle={account && <AccountLine account={account} />} dragRegion={dragRegion}>
        <div className="flex min-w-0 items-center gap-3">
          {account && accounts.data && <SyncStatus accounts={accounts.data} account={account} />}
          <div className="flex shrink-0 items-center gap-1">
            <IconButton icon={Search} label={t("mail.search")} disabled />
            <IconButton icon={SquarePen} label={t("mail.compose")} disabled />
          </div>
        </div>
      </ViewHeader>
      {body}
    </div>
  );
}
