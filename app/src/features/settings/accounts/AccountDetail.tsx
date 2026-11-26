import { ArrowLeft, TriangleExclamationPoint } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { Notice } from "../../../components/Notice";
import { Switch } from "../../../components/Switch";
import { TextField } from "../../../components/TextField";
import type { Account, ServerSettings } from "../../../services/ipc";
import { useUiStore } from "../../../stores/ui";
import { Avatar } from "../../accounts/Avatar";
import { useAccounts, useRemoveAccount, useRetryAccount, useUpdatePassword } from "../../accounts/queries";
import { useSetWritesEnabled } from "../../actions/queries";
import { useSendSettings } from "../../composer/queries";
import { SpecialFolders } from "./SpecialFolders";
import { QuotaBlock } from "./QuotaBlock";
import { OAuthSignInAgain } from "./OAuthSignIn";
import { IdentitiesBlock } from "../../identities/IdentitiesBlock";
import { HiddenAccountBlock } from "../../unified/HiddenAccountBlock";
import { useResolveServerChange, useSettingsSyncStatus } from "../../settings-sync/queries";
import { PasswordFromOtherDevice } from "../../settings-sync/PasswordTransfer";

function Block({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6">
      <div className="flex flex-col gap-1">
        <h3 className="text-14 font-semibold text-primary">{title}</h3>
        {description && <p className="text-13 text-secondary">{description}</p>}
      </div>
      {children}
    </section>
  );
}

function formatDateTime(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(ms);
}

function StatusBlock({ account }: { account: Account }) {
  const { t, i18n } = useTranslation();
  const retry = useRetryAccount();
  const { status } = account;
  const stateText = t(`settings.accounts.detail.states.${status.state}`);
  return (
    <Block title={t("settings.accounts.detail.status")}>
      <p className="text-14 text-primary" data-testid="account-state">
        {stateText}
      </p>
      <p className="text-13 text-secondary">
        {status.lastSyncAt === null
          ? t("settings.accounts.detail.neverSynced")
          : t("settings.accounts.detail.lastSync", { time: formatDateTime(status.lastSyncAt, i18n.language) })}
      </p>
      {status.error && (status.state === "offline" || status.state === "error") && (
        <ErrorNotice
          context={stateText}
          error={status.error}
          onRetry={() => {
            retry.mutate(account.id);
          }}
        />
      )}
      {retry.isError && <ErrorNotice context={t("status.retryFailed")} error={retry.error} />}
    </Block>
  );
}

/**
 * The per-account switch for changes on the server (stage 4). Off for every account until the
 * user turns it on here; the explanation says what it allows and that it applies to this account.
 */
function WritesBlock({ account }: { account: Account }) {
  const { t } = useTranslation();
  const save = useSetWritesEnabled();
  // While the core stores it, the switch shows the requested position.
  const checked = save.isPending ? save.variables.enabled : account.writesEnabled;
  return (
    <Block title={t("settings.accounts.detail.writes.title")}>
      <Switch
        label={t("settings.accounts.detail.writes.label")}
        description={t("settings.accounts.detail.writes.description")}
        checked={checked}
        disabled={save.isPending}
        onChange={(enabled) => {
          save.mutate({ accountId: account.id, enabled });
        }}
        data-testid="writes-switch"
      />
      <p role="status" className="text-13 text-secondary" data-testid="writes-state">
        {checked ? t("settings.accounts.detail.writes.on") : t("settings.accounts.detail.writes.off")}
      </p>
      {checked && <p className="text-12 text-secondary">{t("settings.accounts.detail.writes.offNote")}</p>}
      {save.isError && <ErrorNotice context={t("settings.accounts.detail.writes.failed")} error={save.error} />}
    </Block>
  );
}

function ServerRow({ label, server }: { label: string; server: ServerSettings }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col">
      <dt className="text-12 text-secondary">{label}</dt>
      <dd className="text-13 text-primary select-text">
        {t("settings.accounts.setup.serverSummary", {
          host: server.host,
          port: String(server.port),
          security: t(`settings.accounts.setup.securityOptions.${server.security}`),
        })}
      </dd>
      <dd className="text-12 text-secondary select-text">
        {t("settings.accounts.setup.usernameValue", { username: server.username })}
      </dd>
    </div>
  );
}

/**
 * Decision 0040 (security review F1): another device in the settings sync set other servers for
 * this account. The core keeps using the current ones, and the stored password stays with them,
 * until the user takes the new servers (writing off, password asked for again) or keeps these.
 */
function ServerChangeNotice({ account }: { account: Account }) {
  const { t } = useTranslation();
  const status = useSettingsSyncStatus();
  const resolve = useResolveServerChange();
  const change = status.data?.serverChanges.find((c) => c.accountId === account.id);
  if (!change) return null;
  const answer = (apply: boolean) => {
    resolve.mutate({ accountId: account.id, apply });
  };
  return (
    <Notice tone="warning" title={t("settings.accounts.detail.serverChange.title")} data-testid="server-change">
      <p>{t("settings.accounts.detail.serverChange.body")}</p>
      <dl className="flex flex-col gap-3">
        <ServerRow label={t("settings.accounts.detail.serverChange.incoming")} server={change.imap} />
        <ServerRow label={t("settings.accounts.detail.serverChange.outgoing")} server={change.smtp} />
      </dl>
      <p className="text-12">{t("settings.accounts.detail.serverChange.applyNote")}</p>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          disabled={resolve.isPending}
          onClick={() => {
            answer(true);
          }}
        >
          {resolve.isPending && resolve.variables.apply
            ? t("settings.accounts.detail.serverChange.applying")
            : t("settings.accounts.detail.serverChange.apply")}
        </Button>
        <Button
          variant="primary"
          disabled={resolve.isPending}
          onClick={() => {
            answer(false);
          }}
        >
          {t("settings.accounts.detail.serverChange.keep")}
        </Button>
      </div>
      {resolve.isError && <ErrorNotice context={t("settings.accounts.detail.serverChange.failed")} error={resolve.error} />}
    </Notice>
  );
}

function PasswordChange({ account }: { account: Account }) {
  const { t } = useTranslation();
  // Stage 14: a JMAP account with an API token replaces its token here.
  const texts = account.credential === "apiToken" ? "apiToken" : "password";
  const [password, setPassword] = useState("");
  const update = useUpdatePassword();
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!password) return;
    update.mutate(
      { accountId: account.id, password },
      {
        onSuccess: () => {
          setPassword("");
        },
      },
    );
  };
  return (
    <Block title={t(`settings.accounts.detail.${texts}.title`)} description={t(`settings.accounts.detail.${texts}.description`)}>
      <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-3" data-testid="password-form">
        <TextField
          type="password"
          label={t(`settings.accounts.detail.${texts}.label`)}
          value={password}
          onChange={(next) => {
            setPassword(next);
            update.reset();
          }}
          data-testid="new-password"
        />
        <div>
          <Button type="submit" variant="secondary" disabled={!password || update.isPending}>
            {update.isPending ? t(`settings.accounts.detail.${texts}.saving`) : t(`settings.accounts.detail.${texts}.save`)}
          </Button>
        </div>
        {update.isSuccess && <Notice tone="success" role="status" title={t(`settings.accounts.detail.${texts}.saved`)} />}
        {update.isError && <ErrorNotice context={t(`settings.accounts.detail.${texts}.failed`)} error={update.error} />}
      </form>
    </Block>
  );
}

/**
 * Removing an account deletes local data and the keychain entry, which no undo can bring back.
 * DESIGN.md keeps dialogs for permanently deleting mail, so this asks inline, in place: one more
 * deliberate click, the consequences spelled out, nothing modal.
 */
function RemoveAccount({ account }: { account: Account }) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const remove = useRemoveAccount();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const confirmRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (confirming) confirmRef.current?.focus();
  }, [confirming]);

  return (
    <Block title={t("settings.accounts.detail.remove.title")} description={t("settings.accounts.detail.remove.description")}>
      {confirming ? (
        <div
          ref={confirmRef}
          tabIndex={-1}
          role="group"
          aria-labelledby="remove-account-title"
          // Design pass 23.11.2026: like InlineConfirm, a raised card with a danger bar and symbol.
          className="relative flex flex-col gap-3 overflow-hidden rounded-card border border-border-subtle bg-surface-raised p-4 ps-5 shadow-card before:absolute before:inset-y-0 before:start-0 before:w-1 before:bg-danger"
          data-testid="remove-confirm"
        >
          <p id="remove-account-title" className="flex items-start gap-2 font-semibold text-primary">
            <TriangleExclamationPoint aria-hidden className="size-(--ui-icon) shrink-0 text-danger" strokeWidth={1.75} />
            <span>{t("settings.accounts.detail.remove.confirmTitle", { email: account.email })}</span>
          </p>
          <p className="text-secondary">{t("settings.accounts.detail.remove.confirmBody")}</p>
          <div className="flex gap-2">
            <Button
              variant="danger"
              disabled={remove.isPending}
              onClick={() => {
                // Back to the list after the promise, not in a `mutate` callback: the core
                // announces the removal (`accountsChanged`) before it answers, the refreshed
                // list no longer has the account, and the account page unmounts this component.
                // React Query drops the callbacks of an unmounted observer. Only if the user is
                // still on this account's page; a failure is shown below (`remove.isError`).
                remove.mutateAsync(account.id).then(
                  () => {
                    const { view, accountsPage } = useUiStore.getState();
                    const stillHere =
                      view.kind === "settings" && accountsPage.kind === "account" && accountsPage.accountId === account.id;
                    if (stillHere) showAccountsPage({ kind: "list" });
                  },
                  () => undefined,
                );
              }}
            >
              {remove.isPending ? t("settings.accounts.detail.remove.removing") : t("settings.accounts.detail.remove.confirm")}
            </Button>
            <Button
              variant="secondary"
              disabled={remove.isPending}
              onClick={() => {
                setConfirming(false);
                remove.reset();
              }}
            >
              {t("settings.accounts.detail.remove.cancel")}
            </Button>
          </div>
          {remove.isError && <ErrorNotice context={t("settings.accounts.detail.remove.failed")} error={remove.error} />}
        </div>
      ) : (
        <div>
          <Button
            variant="secondary"
            onClick={() => {
              setConfirming(true);
            }}
          >
            {t("settings.accounts.detail.remove.action")}
          </Button>
        </div>
      )}
    </Block>
  );
}

/** One account: status, read-only note and the switch for changes, servers, password, special folders, removal. */
/**
 * Stage 14 (decision 0034): what is different on Gmail (labels, archive, Sent copies), shown
 * once the core knows the account is Gmail (by host, or `X-GM-EXT-1` at the last login).
 */
function GmailNote({ account }: { account: Account }) {
  const { t } = useTranslation();
  const send = useSendSettings(account.id);
  if (!send.data?.gmail) return null;
  return (
    <Notice title={t("settings.accounts.detail.gmail.title")} data-testid="gmail-account">
      <p>{t("settings.accounts.detail.gmail.body")}</p>
      {send.data.sentCopy === "app" && <p>{t("settings.accounts.detail.gmail.customCopy")}</p>}
    </Notice>
  );
}

export function AccountDetail({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const account = accounts.data?.find((a) => a.id === accountId);

  const back = (
    // Stage 21 (decision 0071): the quiet button's arrow on the page's edge, not its padding.
    <div className="-ms-3">
      <Button
        variant="quiet"
        onClick={() => {
          showAccountsPage({ kind: "list" });
        }}
      >
        <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        {t("settings.accounts.back")}
      </Button>
    </div>
  );

  if (!account) {
    return (
      <div className="flex max-w-(--ui-content-max-width) flex-col gap-6">
        {back}
        {accounts.isError ? (
          <ErrorNotice
            context={t("settings.accounts.loadFailed")}
            error={accounts.error}
            onRetry={() => {
              void accounts.refetch();
            }}
          />
        ) : (
          accounts.isSuccess && <p className="text-14 text-secondary">{t("errors.codes.not_found.title")}</p>
        )}
      </div>
    );
  }

  return (
    <section aria-labelledby="account-title" className="flex max-w-(--ui-content-max-width) flex-col gap-6" data-testid="account-detail">
      {back}
      <div className="flex items-center gap-3">
        <Avatar name={account.displayName} email={account.email} accountColor={account.color} />
        <div className="flex min-w-0 flex-col">
          <h2 id="account-title" className="truncate text-20 font-semibold text-primary">
            {account.displayName || account.email}
          </h2>
          <p className="truncate text-13 text-secondary select-text">{account.email}</p>
        </div>
        {!account.writesEnabled && (
          <span className="ms-auto shrink-0 rounded-control border border-border-strong px-2 text-12 font-medium text-secondary">
            {t("settings.accounts.readOnly")}
          </span>
        )}
      </div>
      {!account.writesEnabled && (
        <Notice title={t("settings.accounts.setup.readOnlyAccountTitle")} data-testid="read-only-account">
          <p>{t("settings.accounts.setup.readOnlyAccountBody")}</p>
        </Notice>
      )}
      <ServerChangeNotice account={account} />
      <StatusBlock account={account} />
      {/* Stage 20 (decision 0060): how full the mailbox on the server is. */}
      <QuotaBlock account={account} />
      <WritesBlock account={account} />
      <Block title={t("settings.accounts.detail.servers")}>
        <dl className="flex flex-col gap-3">
          {/* Stage 14 (decision 0041): a JMAP account has one server for everything. */}
          {account.protocol === "jmap" ? (
            <ServerRow label={t("settings.accounts.setup.jmap.server")} server={account.imap} />
          ) : (
            <>
              <ServerRow label={t("settings.accounts.setup.incoming")} server={account.imap} />
              <ServerRow label={t("settings.accounts.setup.outgoing")} server={account.smtp} />
            </>
          )}
        </dl>
      </Block>
      <GmailNote account={account} />
      {/* Stage 22: an account the settings sync brought can take its password from another device. */}
      <PasswordFromOtherDevice account={account} />
      {/* Stage 14: an account that signs in with its provider has no password. */}
      {account.oauth ? <OAuthSignInAgain account={account} /> : <PasswordChange account={account} />}
      {/* Stage 7: sender addresses, and whether the account shows in the unified inbox. */}
      <Block title={t("identities.title")} description={t("identities.description")}>
        <IdentitiesBlock account={account} />
      </Block>
      <Block title={t("hiddenAccount.title")}>
        <HiddenAccountBlock account={account} />
      </Block>
      <Block
        title={t("settings.accounts.detail.folders.title")}
        description={t("settings.accounts.detail.folders.description")}
      >
        <SpecialFolders account={account} />
      </Block>
      <RemoveAccount account={account} />
    </section>
  );
}
