import { ChevronRight, UserPlus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import type { Account } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { Avatar } from "../accounts/Avatar";
import { useAccounts } from "../accounts/queries";
import { SyncDetectedHint } from "../settings-sync/SyncDetectedHint";
import { AccountDetail } from "./accounts/AccountDetail";
import { AddAccount } from "./accounts/AddAccount";
import { AccountGroupsSection } from "../unified/AccountGroupsSection";

function AccountRow({ account }: { account: Account }) {
  const { t } = useTranslation();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  return (
    <li>
      <button
        type="button"
        onClick={() => {
          showAccountsPage({ kind: "account", accountId: account.id });
        }}
        data-testid={`account-row-${account.id}`}
        className="flex w-full items-center gap-3 rounded-card px-3 py-2 text-left transition-colors hover:bg-hover"
      >
        <Avatar name={account.displayName} email={account.email} accountColor={account.color} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-14 font-medium text-primary">{account.displayName || account.email}</span>
          <span className="truncate text-12 text-secondary">
            {t("settings.accounts.rowStatus", {
              email: account.email,
              state: t(`settings.accounts.detail.states.${account.status.state}`),
            })}
          </span>
        </span>
        {!account.writesEnabled && (
          <span className="shrink-0 rounded-control border border-border-strong px-2 text-12 font-medium text-secondary">
            {t("settings.accounts.readOnly")}
          </span>
        )}
        <ChevronRight aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      </button>
    </li>
  );
}

function AccountList() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const add = () => {
    showAccountsPage({ kind: "add" });
  };
  return (
    <SettingsSection title={t("settings.accounts.title")}>
      {/* Stage 11: a settings sync found in an account on this (second) device. */}
      <SyncDetectedHint />
      {accounts.isError && (
        <ErrorNotice
          context={t("settings.accounts.loadFailed")}
          error={accounts.error}
          onRetry={() => {
            void accounts.refetch();
          }}
        />
      )}
      {accounts.data?.length === 0 && (
        <div className="flex rounded-card border border-border bg-canvas">
          <EmptyState
            icon={UserPlus}
            title={t("settings.accounts.empty.title")}
            body={t("settings.accounts.empty.body")}
            action={{ label: t("settings.accounts.add"), onClick: add }}
          />
        </div>
      )}
      {accounts.data && accounts.data.length > 0 && (
        <>
          <ul aria-label={t("settings.accounts.title")} className="flex flex-col gap-1">
            {accounts.data.map((account) => (
              <AccountRow key={account.id} account={account} />
            ))}
          </ul>
          <div>
            <Button onClick={add}>{t("settings.accounts.add")}</Button>
          </div>
          {/* Stage 7: groups for the unified inbox. */}
          <AccountGroupsSection accounts={accounts.data} />
        </>
      )}
    </SettingsSection>
  );
}

/** Settings → Accounts: the list, the setup form or one account, all as pages of this view. */
export function AccountsSettings() {
  const page = useUiStore((state) => state.accountsPage);
  switch (page.kind) {
    case "add":
      return <AddAccount />;
    case "account":
      return <AccountDetail accountId={page.accountId} />;
    case "list":
      return <AccountList />;
  }
}
