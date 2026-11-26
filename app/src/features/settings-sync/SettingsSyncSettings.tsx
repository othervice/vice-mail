import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { SettingsSection } from "../../components/SettingsSection";
import { TextField } from "../../components/TextField";
import { SETTINGS_SYNC_FOLDER, type Account, type AccountId, type SettingsSyncStatus, type SyncDevice, type SyncQuestion } from "../../services/ipc";
import { useAccounts, useUpdatePassword } from "../accounts/queries";
import { InlineConfirm } from "../mcp/InlineConfirm";
import { AccountPicker } from "./AccountPicker";
import { formatSyncTime } from "./format";
import { JoinFlow } from "./JoinFlow";
import { GivePasswords, TakeOverPasswords } from "./PasswordTransfer";
import { EMPTY_PASSPHRASE, PassphraseFields, passphraseProblem, type PassphraseDraft } from "./PassphraseFields";
import {
  settingsSyncQueryKey,
  useChangeSettingsSyncPassphrase,
  useDisableSettingsSync,
  useEnableSettingsSync,
  useResetSettingsSync,
  useResolveSyncQuestion,
  useSetSyncDeviceName,
  useSettingsSyncStatus,
  useSyncSettingsNow,
} from "./queries";
import { cx } from "../../components/cx";
import { SETTING_ANCHOR_CLASS, SETTING_BLOCK, settingAnchor } from "../../components/settingAnchor";

const MAX_DEVICE_NAME_CHARS = 64;

function Block({
  title,
  description,
  children,
  ...rest
}: {
  title: string;
  description?: string;
  children: ReactNode;
  "data-testid"?: string;
}) {
  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6" data-testid={rest["data-testid"]} {...SETTING_BLOCK}>
      {/* Stage 22 (decision 0073): found by the settings search with its description. */}
      <div {...settingAnchor(title)} className={cx("flex flex-col gap-1", SETTING_ANCHOR_CLASS)}>
        <h3 className="text-14 font-semibold text-primary">{title}</h3>
        {description && <p className="text-13 text-secondary">{description}</p>}
      </div>
      {children}
    </section>
  );
}

function accountName(account: Account | undefined, fallback: string): string {
  return account ? account.email : fallback;
}

// ---- Off: turn it on, or join a sync another device set up ----

function EnableForm({ accounts, onJoin }: { accounts: readonly Account[]; onJoin: (accountId: AccountId) => void }) {
  const { t } = useTranslation();
  const [accountId, setAccountId] = useState<AccountId | undefined>(accounts[0]?.id);
  const [draft, setDraft] = useState<PassphraseDraft>(EMPTY_PASSPHRASE);
  const [showErrors, setShowErrors] = useState(false);
  const enable = useEnableSettingsSync();
  const selected = accounts.some((account) => account.id === accountId) ? accountId : accounts[0]?.id;
  if (!selected) return null;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    setShowErrors(true);
    if (passphraseProblem(draft)) return;
    enable.mutate(
      { accountId: selected, passphrase: draft.passphrase },
      {
        onError: (error) => {
          if (error.code === "sync_already_set_up") onJoin(selected);
        },
      },
    );
  };

  return (
    <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-4" data-testid="sync-enable-form">
      <AccountPicker
        label={t("settingsSync.enable.account")}
        description={t("settingsSync.enable.accountDescription", { folder: SETTINGS_SYNC_FOLDER })}
        value={selected}
        options={accounts.map((account) => ({ value: account.id, label: account.email }))}
        onChange={(next) => {
          setAccountId(next);
          enable.reset();
        }}
      />
      <PassphraseFields
        draft={draft}
        onChange={(next) => {
          setDraft(next);
          enable.reset();
        }}
        showErrors={showErrors}
        data-testid="sync-enable"
      />
      <p className="text-13 text-secondary">{t("settingsSync.passphrase.forget")}</p>
      <div className="flex flex-wrap gap-2">
        <Button type="submit" disabled={enable.isPending} data-testid="sync-enable-submit">
          {enable.isPending ? t("settingsSync.enable.enabling") : t("settingsSync.enable.submit")}
        </Button>
        <Button
          variant="quiet"
          onClick={() => {
            onJoin(selected);
          }}
          data-testid="sync-enable-join-instead"
        >
          {t("settingsSync.enable.joinInstead")}
        </Button>
      </div>
      {enable.isError && enable.error.code !== "sync_already_set_up" && (
        <ErrorNotice context={t("settingsSync.enable.failed")} error={enable.error} />
      )}
    </form>
  );
}

type OffMode = { kind: "enable" } | { kind: "join"; accountId: AccountId; alreadySetUp: boolean };

function OffPage({ status, accounts }: { status: SettingsSyncStatus; accounts: readonly Account[] }) {
  const { t } = useTranslation();
  const detected = status.detectedAccounts.filter((id) => accounts.some((account) => account.id === id));
  const [mode, setMode] = useState<OffMode | null>(null);
  const firstDetected = detected[0];
  const current: OffMode = mode ?? (firstDetected ? { kind: "join", accountId: firstDetected, alreadySetUp: false } : { kind: "enable" });

  if (accounts.length === 0) {
    return <p className="text-13 text-secondary">{t("settingsSync.off.noAccounts")}</p>;
  }

  if (current.kind === "join") {
    const account = accounts.find((a) => a.id === current.accountId);
    const email = accountName(account, t("settingsSync.unknownAccount"));
    return (
      <Block title={t("settingsSync.join.title")}>
        {current.alreadySetUp && (
          <Notice tone="info" title={t("errors.codes.sync_already_set_up.title")} data-testid="sync-already-set-up">
            <p>{t("errors.codes.sync_already_set_up.hint")}</p>
          </Notice>
        )}
        {detected.length > 1 && (
          <AccountPicker
            label={t("settingsSync.join.account")}
            value={current.accountId}
            options={detected.map((id) => ({ value: id, label: accountName(accounts.find((a) => a.id === id), id) }))}
            onChange={(accountId) => {
              setMode({ kind: "join", accountId, alreadySetUp: false });
            }}
          />
        )}
        <JoinFlow
          key={current.accountId}
          accountId={current.accountId}
          intro={t("settingsSync.join.intro", { email, folder: SETTINGS_SYNC_FOLDER })}
          onCancel={() => {
            setMode({ kind: "enable" });
          }}
        />
      </Block>
    );
  }

  return (
    <>
      {firstDetected && (
        <Notice tone="info" title={t("settingsSync.off.detectedTitle")} data-testid="sync-detected">
          <p>
            {t("settingsSync.off.detectedBody", {
              email: accountName(accounts.find((a) => a.id === firstDetected), t("settingsSync.unknownAccount")),
            })}
          </p>
          <div>
            <Button
              variant="secondary"
              onClick={() => {
                setMode({ kind: "join", accountId: firstDetected, alreadySetUp: false });
              }}
            >
              {t("settingsSync.off.joinDetected")}
            </Button>
          </div>
        </Notice>
      )}
      <Block title={t("settingsSync.enable.title")} description={t("settingsSync.enable.description")}>
        <EnableForm
          accounts={accounts}
          onJoin={(accountId) => {
            setMode({ kind: "join", accountId, alreadySetUp: true });
          }}
        />
      </Block>
    </>
  );
}

// ---- On: status, passwords, questions, devices, passphrase, reset, off ----

function StatusBlock({ status, account }: { status: SettingsSyncStatus; account: Account | undefined }) {
  const { t, i18n } = useTranslation();
  const syncNow = useSyncSettingsNow();
  return (
    <Block title={t("settingsSync.status.title")} data-testid="sync-status">
      <p className="text-14 text-primary" data-testid="sync-state">
        {t(`settingsSync.status.states.${status.state}`)}
      </p>
      <dl className="flex flex-col gap-1 text-13 text-secondary">
        <div className="flex flex-wrap gap-x-2">
          <dt>{t("settingsSync.status.account")}</dt>
          <dd className="text-primary select-text">{accountName(account, t("settingsSync.unknownAccount"))}</dd>
        </div>
        {status.folder && (
          <div className="flex flex-wrap gap-x-2">
            <dt>{t("settingsSync.status.folder")}</dt>
            <dd className="font-mono text-primary select-text">{status.folder}</dd>
          </div>
        )}
        <div className="flex flex-wrap gap-x-2">
          <dt>{t("settingsSync.status.lastSync")}</dt>
          <dd className="text-primary" data-testid="sync-last">
            {status.lastSyncAt === null ? t("settingsSync.status.never") : formatSyncTime(status.lastSyncAt, i18n.language)}
          </dd>
        </div>
        {status.pendingChanges > 0 && (
          <div className="flex flex-wrap gap-x-2">
            <dt>{t("settingsSync.status.pending")}</dt>
            <dd className="text-primary tabular-nums">{status.pendingChanges}</dd>
          </div>
        )}
      </dl>
      {status.rejectedEntries > 0 && (
        <Notice tone="warning" title={t("settingsSync.status.rejectedTitle", { count: status.rejectedEntries })} data-testid="sync-rejected">
          <p>{t("settingsSync.status.rejectedBody")}</p>
        </Notice>
      )}
      {status.state === "error" && status.lastError && (
        <ErrorNotice
          context={t("settingsSync.status.errorContext")}
          error={status.lastError}
          onRetry={() => {
            syncNow.mutate(undefined);
          }}
          data-testid="sync-last-error"
        />
      )}
      {status.state !== "needsPassphrase" && (
        <div>
          <Button
            variant="secondary"
            disabled={syncNow.isPending}
            onClick={() => {
              syncNow.mutate(undefined);
            }}
            data-testid="sync-now"
          >
            {syncNow.isPending ? t("settingsSync.status.syncing") : t("settingsSync.status.syncNow")}
          </Button>
        </div>
      )}
      {syncNow.isError && <ErrorNotice context={t("settingsSync.status.syncFailed")} error={syncNow.error} />}
    </Block>
  );
}

function NeedsPassphrase({ status, account }: { status: SettingsSyncStatus; account: Account | undefined }) {
  const { t } = useTranslation();
  if (!status.accountId) return null;
  return (
    <Block title={t("settingsSync.needsPassphrase.title")} data-testid="sync-needs-passphrase">
      {status.lastError && <ErrorNotice context={t("settingsSync.needsPassphrase.context")} error={status.lastError} />}
      <JoinFlow
        accountId={status.accountId}
        intro={t("settingsSync.needsPassphrase.intro", { email: accountName(account, t("settingsSync.unknownAccount")) })}
      />
    </Block>
  );
}

function PasswordRow({ accountId, account }: { accountId: AccountId; account: Account | undefined }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [password, setPassword] = useState("");
  const update = useUpdatePassword();
  const email = accountName(account, t("settingsSync.unknownAccount"));
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!password) return;
    update.mutate(
      { accountId, password },
      {
        onSuccess: () => {
          setPassword("");
          // The core drops the account from `accountsNeedingPassword`.
          void queryClient.invalidateQueries({ queryKey: settingsSyncQueryKey });
        },
      },
    );
  };
  return (
    <li>
      <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-2" data-testid={`sync-password-${accountId}`}>
        <TextField
          type="password"
          autoComplete="current-password"
          label={t("settingsSync.passwords.label", { email })}
          value={password}
          onChange={(next) => {
            setPassword(next);
            update.reset();
          }}
        />
        <div>
          <Button type="submit" variant="secondary" disabled={!password || update.isPending}>
            {update.isPending ? t("settingsSync.passwords.saving") : t("settingsSync.passwords.save")}
          </Button>
        </div>
        {update.isError && <ErrorNotice context={t("settingsSync.passwords.failed", { email })} error={update.error} />}
      </form>
    </li>
  );
}

function PasswordsBlock({ status, accounts }: { status: SettingsSyncStatus; accounts: readonly Account[] }) {
  const { t } = useTranslation();
  // An account the list does not have yet arrives with `accountsChanged`; until then it has no row.
  const waiting = accounts.filter((account) => status.accountsNeedingPassword.includes(account.id));
  // Stage 22: how a transfer ended stays visible after the last password arrived.
  if (waiting.length === 0 && !status.passwordTransfer) return null;
  return (
    <Block title={t("settingsSync.passwords.title")} description={t("settingsSync.passwords.description")} data-testid="sync-passwords">
      {(waiting.some((account) => !account.oauth) || status.passwordTransfer) && (
        <TakeOverPasswords transfer={status.passwordTransfer} accounts={accounts} />
      )}
      <ul className="flex flex-col gap-4">
        {waiting.map((account) => (
          <PasswordRow key={account.id} accountId={account.id} account={account} />
        ))}
      </ul>
    </Block>
  );
}

function QuestionRow({ question }: { question: SyncQuestion }) {
  const { t } = useTranslation();
  const resolve = useResolveSyncQuestion();
  const [confirming, setConfirming] = useState(false);
  const error = resolve.isError ? <ErrorNotice context={t("settingsSync.questions.failed")} error={resolve.error} /> : null;
  return (
    <li className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4" data-testid={`sync-question-${question.accountId}`}>
      <p className="text-14 text-primary">{t("settingsSync.questions.accountRemoved", { email: question.email })}</p>
      {confirming ? (
        <InlineConfirm
          title={t("settingsSync.questions.confirmTitle", { email: question.email })}
          confirmLabel={t("settingsSync.questions.remove")}
          cancelLabel={t("settingsSync.questions.cancel")}
          busy={resolve.isPending}
          onConfirm={() => {
            resolve.mutate({ accountId: question.accountId, remove: true });
          }}
          onCancel={() => {
            setConfirming(false);
            resolve.reset();
          }}
          footer={error}
          data-testid="sync-question-confirm"
        >
          <p>{t("settingsSync.questions.confirmBody")}</p>
        </InlineConfirm>
      ) : (
        <>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              disabled={resolve.isPending}
              onClick={() => {
                setConfirming(true);
              }}
              data-testid="sync-question-remove"
            >
              {t("settingsSync.questions.remove")}
            </Button>
            <Button
              variant="secondary"
              disabled={resolve.isPending}
              onClick={() => {
                resolve.mutate({ accountId: question.accountId, remove: false });
              }}
              data-testid="sync-question-keep"
            >
              {t("settingsSync.questions.keep")}
            </Button>
          </div>
          <p className="text-12 text-secondary">{t("settingsSync.questions.keepHint")}</p>
          {error}
        </>
      )}
    </li>
  );
}

function QuestionsBlock({ questions }: { questions: readonly SyncQuestion[] }) {
  const { t } = useTranslation();
  if (questions.length === 0) return null;
  return (
    <Block title={t("settingsSync.questions.title")} data-testid="sync-questions">
      <ul className="flex flex-col gap-3">
        {questions.map((question) => (
          <QuestionRow key={question.accountId} question={question} />
        ))}
      </ul>
    </Block>
  );
}

function DeviceName({ device }: { device: SyncDevice }) {
  const { t } = useTranslation();
  const [name, setName] = useState(device.name);
  const [showErrors, setShowErrors] = useState(false);
  const save = useSetSyncDeviceName();
  const trimmed = name.trim();
  const error = !trimmed
    ? t("settingsSync.devices.nameRequired")
    : [...trimmed].length > MAX_DEVICE_NAME_CHARS
      ? t("settingsSync.devices.nameTooLong", { max: MAX_DEVICE_NAME_CHARS })
      : null;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setShowErrors(true);
    if (error) return;
    save.mutate(trimmed);
  };
  return (
    <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-2" data-testid="sync-device-name">
      <TextField
        label={t("settingsSync.devices.nameLabel")}
        description={t("settingsSync.devices.nameDescription")}
        value={name}
        onChange={(next) => {
          setName(next);
          save.reset();
        }}
        error={showErrors ? error : null}
        data-testid="sync-device-name-field"
      />
      <div>
        <Button type="submit" variant="secondary" disabled={save.isPending || trimmed === device.name}>
          {t("settingsSync.devices.saveName")}
        </Button>
      </div>
      {save.isSuccess && <Notice tone="success" role="status" title={t("settingsSync.devices.nameSaved")} />}
      {save.isError && <ErrorNotice context={t("settingsSync.devices.nameFailed")} error={save.error} />}
    </form>
  );
}

function DevicesBlock({ devices }: { devices: readonly SyncDevice[] }) {
  const { t, i18n } = useTranslation();
  const current = devices.find((device) => device.current);
  return (
    <Block title={t("settingsSync.devices.title")} data-testid="sync-devices">
      {devices.length === 0 ? (
        <p className="text-13 text-secondary">{t("settingsSync.devices.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-1" aria-label={t("settingsSync.devices.title")}>
          {devices.map((device) => (
            <li key={device.id} className="flex flex-wrap items-baseline gap-x-2 text-13" data-testid={`sync-device-${device.id}`}>
              <span className="font-medium text-primary">{device.name}</span>
              {device.current && <span className="text-12 text-accent">{t("settingsSync.devices.thisDevice")}</span>}
              <span className="text-secondary">
                {t("settingsSync.devices.detail", { system: device.system, time: formatSyncTime(device.lastSeenAt, i18n.language) })}
              </span>
            </li>
          ))}
        </ul>
      )}
      {current && <DeviceName key={current.name} device={current} />}
    </Block>
  );
}

function ChangePassphraseBlock() {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<PassphraseDraft>(EMPTY_PASSPHRASE);
  const [showErrors, setShowErrors] = useState(false);
  const change = useChangeSettingsSyncPassphrase();
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setShowErrors(true);
    if (passphraseProblem(draft)) return;
    change.mutate(draft.passphrase, {
      onSuccess: () => {
        setDraft(EMPTY_PASSPHRASE);
        setShowErrors(false);
      },
    });
  };
  return (
    <Block title={t("settingsSync.change.title")} description={t("settingsSync.change.description")} data-testid="sync-change">
      <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-3">
        <PassphraseFields
          draft={draft}
          onChange={(next) => {
            setDraft(next);
            change.reset();
          }}
          showErrors={showErrors}
          variant="new"
          data-testid="sync-change"
        />
        <div>
          <Button type="submit" variant="secondary" disabled={change.isPending} data-testid="sync-change-submit">
            {change.isPending ? t("settingsSync.change.changing") : t("settingsSync.change.submit")}
          </Button>
        </div>
        {change.isSuccess && <Notice tone="success" role="status" title={t("settingsSync.change.done")} />}
        {change.isError && <ErrorNotice context={t("settingsSync.change.failed")} error={change.error} />}
      </form>
    </Block>
  );
}

function ResetBlock() {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<PassphraseDraft>(EMPTY_PASSPHRASE);
  const [showErrors, setShowErrors] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const reset = useResetSettingsSync();
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setShowErrors(true);
    if (passphraseProblem(draft)) return;
    setConfirming(true);
  };
  const failure = reset.isError ? <ErrorNotice context={t("settingsSync.reset.failed")} error={reset.error} /> : null;
  return (
    <Block title={t("settingsSync.reset.title")} description={t("settingsSync.reset.description")} data-testid="sync-reset">
      {confirming ? (
        <InlineConfirm
          title={t("settingsSync.reset.confirmTitle")}
          confirmLabel={t("settingsSync.reset.confirm")}
          cancelLabel={t("settingsSync.reset.cancel")}
          busy={reset.isPending}
          onConfirm={() => {
            reset.mutate(draft.passphrase, {
              onSuccess: () => {
                setDraft(EMPTY_PASSPHRASE);
                setShowErrors(false);
                setConfirming(false);
              },
            });
          }}
          onCancel={() => {
            setConfirming(false);
            reset.reset();
          }}
          footer={failure}
          data-testid="sync-reset-confirm"
        >
          <p>{t("settingsSync.reset.confirmBody")}</p>
        </InlineConfirm>
      ) : (
        <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-3">
          <PassphraseFields
            draft={draft}
            onChange={(next) => {
              setDraft(next);
              reset.reset();
            }}
            showErrors={showErrors}
            variant="new"
            data-testid="sync-reset"
          />
          <div>
            <Button type="submit" variant="secondary" data-testid="sync-reset-submit">
              {t("settingsSync.reset.submit")}
            </Button>
          </div>
          {reset.isSuccess && <Notice tone="success" role="status" title={t("settingsSync.reset.done")} />}
        </form>
      )}
    </Block>
  );
}

function DisableBlock() {
  const { t } = useTranslation();
  const disable = useDisableSettingsSync();
  return (
    <Block title={t("settingsSync.disable.title")} description={t("settingsSync.disable.description")} data-testid="sync-disable">
      <div>
        <Button
          variant="secondary"
          disabled={disable.isPending}
          onClick={() => {
            disable.mutate(undefined);
          }}
          data-testid="sync-disable-submit"
        >
          {disable.isPending ? t("settingsSync.disable.disabling") : t("settingsSync.disable.submit")}
        </Button>
      </div>
      {disable.isError && <ErrorNotice context={t("settingsSync.disable.failed")} error={disable.error} />}
    </Block>
  );
}

function OnPage({ status, accounts }: { status: SettingsSyncStatus; accounts: readonly Account[] }) {
  const account = accounts.find((a) => a.id === status.accountId);
  return (
    <>
      <StatusBlock status={status} account={account} />
      {status.state === "needsPassphrase" && <NeedsPassphrase status={status} account={account} />}
      <PasswordsBlock status={status} accounts={accounts} />
      {/* Stage 22: only where there is something to give, or a request to answer. */}
      {status.state !== "needsPassphrase" &&
        (accounts.some((a) => !a.oauth && !status.accountsNeedingPassword.includes(a.id)) || status.passwordRequests.length > 0) && (
          <GivePasswords status={status} accounts={accounts} />
        )}
      <QuestionsBlock questions={status.questions} />
      <DevicesBlock devices={status.devices} />
      {status.state !== "needsPassphrase" && <ChangePassphraseBlock />}
      <ResetBlock />
      <DisableBlock />
    </>
  );
}

/** Settings → Settings sync: on, off, joining from a second device, and everything while it runs. */
export function SettingsSyncSettings() {
  const { t } = useTranslation();
  const status = useSettingsSyncStatus();
  const accounts = useAccounts();
  return (
    <SettingsSection title={t("settingsSync.title")}>
      <p className="text-14 text-secondary">{t("settingsSync.intro")}</p>
      <p className="text-13 text-secondary">{t("settingsSync.what")}</p>
      {status.isError && (
        <ErrorNotice
          context={t("settingsSync.loadFailed")}
          error={status.error}
          onRetry={() => {
            void status.refetch();
          }}
        />
      )}
      {accounts.isError && (
        <ErrorNotice
          context={t("settingsSync.accountsFailed")}
          error={accounts.error}
          // Stage 8: the section needs the accounts; a way out instead of a dead end.
          onRetry={() => {
            void accounts.refetch();
          }}
        />
      )}
      {status.data &&
        accounts.data &&
        (status.data.state === "off" ? (
          <OffPage status={status.data} accounts={accounts.data} />
        ) : (
          <OnPage status={status.data} accounts={accounts.data} />
        ))}
    </SettingsSection>
  );
}
