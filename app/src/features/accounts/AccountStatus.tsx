import { CloudOff, LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, IpcError } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useRetryAccount } from "./queries";
import { allOffline } from "./selection";

/** Stand-in when the core reports `error` without a reason (the contract says it never does). */
const UNKNOWN_ERROR: IpcError = { code: "internal", message: "", detail: null };

function formatTime(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { timeStyle: "short" }).format(ms);
}

interface SyncStatusProps {
  accounts: readonly Account[];
  account: Account;
}

/**
 * Quiet sync state in the list header: connecting, syncing with progress, offline with the next
 * attempt. Nothing while up to date. Only the state word is a live region, so screen readers
 * hear "Syncing" once instead of every progress step.
 */
export function SyncStatus({ accounts, account }: SyncStatusProps) {
  const { t, i18n } = useTranslation();
  const retry = useRetryAccount();
  const { status } = account;

  if (allOffline(accounts)) {
    return (
      <div className="flex min-w-0 items-center gap-2 text-12 text-secondary" data-testid="sync-status">
        <CloudOff aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
        <span role="status" className="truncate">
          {t("status.allOffline")}
        </span>
      </div>
    );
  }

  let state: string | null = null;
  let detail: string | null = null;
  switch (status.state) {
    case "connecting":
      state = t("status.connecting");
      break;
    case "syncing":
      state = t("status.syncing");
      if (status.progress) {
        const numbers = new Intl.NumberFormat(i18n.language);
        detail = t("status.progress", {
          done: numbers.format(status.progress.done),
          total: numbers.format(status.progress.total),
        });
      }
      break;
    case "offline":
      state = t("status.offline");
      if (status.retryAt !== null) detail = t("status.nextAttempt", { time: formatTime(status.retryAt, i18n.language) });
      break;
    case "error":
      state = t("status.error");
      break;
    case "idle":
      break;
  }

  return (
    <div className="flex min-w-0 items-center gap-2 text-12 text-secondary" data-testid="sync-status">
      {(status.state === "connecting" || status.state === "syncing") && (
        <LoaderCircle aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
      )}
      {status.state === "offline" && <CloudOff aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />}
      <span role="status" className="truncate">
        {state}
      </span>
      {detail && <span className="truncate tabular-nums">{detail}</span>}
      {status.state === "offline" && (
        <Button
          variant="quiet"
          disabled={retry.isPending}
          onClick={() => {
            retry.mutate(account.id);
          }}
        >
          {t("status.retryNow")}
        </Button>
      )}
      {retry.isError && (
        <span role="alert" className="truncate text-danger" title={retry.error.detail ?? retry.error.message}>
          {t("status.retryFailed")}
        </span>
      )}
    </div>
  );
}

/**
 * An account that stopped syncing until the user acts (wrong password, untrusted certificate,
 * unsupported server): the reason where the mail would be, with the ways out.
 */
export function AccountErrorBanner({ account }: { account: Account }) {
  const { t } = useTranslation();
  const retry = useRetryAccount();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  if (account.status.state !== "error") return null;
  const error = account.status.error ?? UNKNOWN_ERROR;
  const openAccount = () => {
    showAccountsPage({ kind: "account", accountId: account.id });
  };
  return (
    <div className="flex flex-col gap-2 px-6 pt-4">
      <ErrorNotice
        context={t("status.errorContext", { account: account.displayName || account.email })}
        error={error}
        data-testid="account-error"
        onRetry={() => {
          retry.mutate(account.id);
        }}
        actions={
          <Button variant="secondary" onClick={openAccount}>
            {error.code === "auth_failed" ? t("status.changePassword") : t("status.accountSettings")}
          </Button>
        }
      />
      {retry.isError && <ErrorNotice context={t("status.retryFailed")} error={retry.error} />}
    </div>
  );
}
