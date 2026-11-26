import { ArrowDown, ArrowUp, ChevronRight, UserPlus } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { Tooltip } from "../../components/Tooltip";
import type { Account } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { accountFill } from "../accounts/accountColor";
import { useAccountArrange } from "../accounts/arrange";
import { useAccounts } from "../accounts/queries";
import { SyncDetectedHint } from "../settings-sync/SyncDetectedHint";
import { AccountDetail } from "./accounts/AccountDetail";
import { AddAccount } from "./accounts/AddAccount";
// Stage 19 (decision 0057): takeover from Thunderbird.
import { ThunderbirdTakeover } from "./accounts/ThunderbirdTakeover";
import { AccountGroupsSection } from "../unified/AccountGroupsSection";

/**
 * Stage 22 (decision 0078): "Move up" and "Move down" of one account in the list, for the keyboard
 * and screen readers (the folder column also takes a dragged account). The focus stays on the
 * button pressed, or goes to the other one at the end of the list; the new place is announced.
 */
function MoveButtons({ account, index, count, onMoved }: { account: Account; index: number; count: number; onMoved: (text: string) => void }) {
  const { t } = useTranslation();
  const arrange = useAccountArrange();
  const name = account.displayName || account.email;
  const move = (step: -1 | 1) => {
    void arrange.move(account.id, step).then((ok) => {
      if (!ok) return;
      const position = index + step + 1;
      onMoved(t("accountEdit.moved", { name, position, count }));
      window.requestAnimationFrame(() => {
        const edge = (step === -1 && position === 1) || (step === 1 && position === count);
        const id = (step === -1) !== edge ? "up" : "down";
        document.querySelector<HTMLElement>(`[data-testid="account-${id}-${account.id}"]`)?.focus();
      });
    });
  };
  const button = (step: -1 | 1) => {
    const up = step === -1;
    const Icon = up ? ArrowUp : ArrowDown;
    const label = t(up ? "accountEdit.moveUpNamed" : "accountEdit.moveDownNamed", { name });
    const disabled = up ? index === 0 : index === count - 1;
    return (
      <Tooltip label={t(up ? "accountEdit.moveUp" : "accountEdit.moveDown")}>
        <button
          type="button"
          aria-label={label}
          disabled={disabled}
          onClick={() => {
            move(step);
          }}
          data-testid={`account-${up ? "up" : "down"}-${account.id}`}
          className="flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-hover hover:text-primary disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-secondary"
        >
          <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </button>
      </Tooltip>
    );
  };
  if (count < 2) return null;
  return (
    <span className="flex shrink-0 items-center gap-1 pe-2">
      {button(-1)}
      {button(1)}
    </span>
  );
}

function AccountRow({ account, index, count, onMoved }: { account: Account; index: number; count: number; onMoved: (text: string) => void }) {
  const { t } = useTranslation();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  return (
    <li className="flex items-center">
      <button
        type="button"
        onClick={() => {
          showAccountsPage({ kind: "account", accountId: account.id });
        }}
        data-testid={`account-row-${account.id}`}
        className="flex min-w-0 flex-1 items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-hover focus-visible:-outline-offset-2"
      >
        {/* Stage 22 (decision 0072): the account's colour as a dot at its name, no initials. */}
        <span className="flex h-(--ui-line-height-14) shrink-0 items-center self-start">
          <span aria-hidden className={cx("size-(--ui-dot) rounded-full", accountFill(account.color))} data-testid="account-dot" />
        </span>
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
          <span className="shrink-0 rounded-full bg-badge px-2 text-12 font-medium text-secondary">
            {t("settings.accounts.readOnly")}
          </span>
        )}
        <ChevronRight aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      </button>
      <MoveButtons account={account} index={index} count={count} onMoved={onMoved} />
    </li>
  );
}

function AccountList() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const [moved, setMoved] = useState("");
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
        <div className="flex rounded-card border border-border-subtle bg-surface-raised shadow-card">
          <EmptyState
            icon={UserPlus}
            title={t("settings.accounts.empty.title")}
            body={t("settings.accounts.empty.body")}
            action={{ label: t("settings.accounts.add"), onClick: add }}
          />
        </div>
      )}
      {accounts.data?.length === 0 && (
        <div>
          <Button
            variant="secondary"
            onClick={() => {
              showAccountsPage({ kind: "thunderbird" });
            }}
            data-testid="accounts-thunderbird"
          >
            {t("takeover.open")}
          </Button>
        </div>
      )}
      {accounts.data && accounts.data.length > 0 && (
        <>
          <ul
            aria-label={t("settings.accounts.title")}
            // Design pass 23.11.2026: the accounts as one raised group, rows split by hairlines.
            className="flex flex-col divide-y divide-border-subtle overflow-hidden rounded-card border border-border-subtle bg-surface-raised shadow-card"
          >
            {accounts.data.map((account, index, all) => (
              <AccountRow key={account.id} account={account} index={index} count={all.length} onMoved={setMoved} />
            ))}
          </ul>
          {/* Stage 22 (decision 0078): the order is every list's order; moving says where to. */}
          {accounts.data.length > 1 && <p className="text-12 text-secondary">{t("accountEdit.orderDescription")}</p>}
          <span aria-live="polite" className="sr-only" data-testid="accounts-moved">
            {moved}
          </span>
          <div className="flex flex-wrap gap-2">
            <Button onClick={add}>{t("settings.accounts.add")}</Button>
            {/* Stage 19 (decision 0057). */}
            <Button
              variant="secondary"
              onClick={() => {
                showAccountsPage({ kind: "thunderbird" });
              }}
              data-testid="accounts-thunderbird"
            >
              {t("takeover.open")}
            </Button>
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
    case "thunderbird":
      return <ThunderbirdTakeover />;
  }
}
