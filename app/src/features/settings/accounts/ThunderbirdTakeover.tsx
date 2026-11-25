import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowLeft, FolderOpen, KeyRound } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { Notice } from "../../../components/Notice";
import {
  takeoverIpc,
  toCoreError,
  type Account,
  type CoreError,
  type AccountId,
  type MailboxId,
  type TakeoverAccount,
  type TakeoverApplied,
  type TakeoverFolder,
  type TakeoverPreview,
} from "../../../services/ipc";
import { useMboxStore } from "../../../stores/mbox";
import { useUiStore } from "../../../stores/ui";
import { useAccounts } from "../../accounts/queries";
import { useMailboxes } from "../../mailboxes/queries";
import { formatSize } from "../../reading/format";
import { AddAccount } from "./AddAccount";

/** One account of the profile: what it has, and "Set up" (the normal setup, prefilled). */
function AccountCard({
  preview,
  account,
  setUp,
  onSetUp,
}: {
  preview: TakeoverPreview;
  account: TakeoverAccount;
  setUp: Account | undefined;
  onSetUp: () => void;
}) {
  const { t } = useTranslation();
  const apply = useMutation({ mutationFn: (accountId: AccountId) => takeoverIpc.applyAccount(preview.token, account.index, accountId) });
  const server = account.imap;
  return (
    <li className="flex flex-col gap-2 rounded-card border border-border-subtle bg-surface-raised p-4 shadow-card" data-testid="takeover-account">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 flex-col">
          <span className="truncate text-14 font-medium text-primary">{account.name || account.email}</span>
          <span className="truncate text-12 text-secondary">
            {account.displayName ? t("takeover.identity", { name: account.displayName, email: account.email }) : account.email}
          </span>
          {server && (
            <span className="truncate text-12 text-secondary select-text">
              {t("takeover.server", { host: server.host, port: server.port })}
            </span>
          )}
        </div>
        {account.kind !== "imap" ? (
          <span className="text-12 text-secondary">{t(`takeover.unsupported.${account.kind}`)}</span>
        ) : setUp ? (
          <Button
            variant="secondary"
            disabled={apply.isPending || apply.isSuccess}
            onClick={() => {
              apply.mutate(setUp.id);
            }}
            data-testid="takeover-apply"
          >
            {t("takeover.applyExisting")}
          </Button>
        ) : (
          <Button onClick={onSetUp} data-testid="takeover-set-up">
            {t("takeover.setUp")}
          </Button>
        )}
      </div>
      {account.wasUnencrypted && <p className="text-12 text-warning">{t("takeover.unencrypted")}</p>}
      {account.identities.length > 0 && (
        <ul aria-label={t("takeover.identities")} className="flex flex-col gap-1">
          {account.identities.map((identity) => (
            <li key={identity.email} className="flex flex-col text-12" data-testid="takeover-identity">
              <span className="text-primary">{identity.name ? t("takeover.identity", { name: identity.name, email: identity.email }) : identity.email}</span>
              {identity.signature && (
                <span className="line-clamp-2 whitespace-pre-line text-secondary">
                  {t("takeover.signature", { text: identity.signature.preview })}
                </span>
              )}
              {identity.signatureUnreadable && <span className="text-warning">{t("takeover.signatureUnreadable")}</span>}
            </li>
          ))}
        </ul>
      )}
      {apply.isSuccess && <Applied applied={apply.data} />}
      {apply.isError && <ErrorNotice context={t("takeover.applyFailed")} error={apply.error} />}
    </li>
  );
}

function Applied({ applied }: { applied: TakeoverApplied }) {
  const { t } = useTranslation();
  return (
    <p role="status" className="text-12 text-success" data-testid="takeover-applied">
      {t("takeover.applied", { identities: applied.identities, signatures: applied.signatures })}
    </p>
  );
}

/** A local folder: into which folder of which account (only accounts that may write). */
function FolderRow({ preview, folder, accounts }: { preview: TakeoverPreview; folder: TakeoverFolder; accounts: Account[] }) {
  const { t, i18n } = useTranslation();
  const accountId = useId();
  const mailboxId = useId();
  const writable = accounts.filter((a) => a.writesEnabled);
  const [account, setAccount] = useState<AccountId | null>(writable[0]?.id ?? null);
  const mailboxes = (useMailboxes(account).data ?? []).filter((m) => m.selectable);
  const [mailbox, setMailbox] = useState<MailboxId | null>(null);
  const target = mailbox ?? mailboxes.find((m) => m.role === "archive")?.id ?? mailboxes[0]?.id ?? null;
  const upsert = useMboxStore((state) => state.upsert);
  const start = useMutation({
    mutationFn: (into: MailboxId) => takeoverIpc.importFolder(preview.token, folder.index, into),
    onSuccess: upsert,
  });
  return (
    <li className="flex flex-col gap-2 rounded-card border border-border-subtle bg-surface-raised p-4 shadow-card" data-testid="takeover-folder">
      <div className="flex items-baseline justify-between gap-2">
        <span className="truncate text-14 font-medium text-primary">{folder.path}</span>
        <span className="shrink-0 text-12 text-secondary tabular-nums">{formatSize(folder.bytes, i18n.language)}</span>
      </div>
      {writable.length === 0 ? (
        <p className="text-12 text-secondary">{t("takeover.folderNeedsWrites")}</p>
      ) : (
        <div className="flex flex-wrap items-end gap-2">
          <label className="flex flex-col gap-1 text-12 text-secondary" htmlFor={accountId}>
            {t("takeover.intoAccount")}
            <select
              id={accountId}
              value={account ?? ""}
              onChange={(e) => {
                setAccount(e.target.value);
                setMailbox(null);
              }}
              className="h-8 rounded-control border border-border bg-surface-raised px-2 text-13 text-primary"
              data-testid="takeover-folder-account"
            >
              {writable.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.email}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-12 text-secondary" htmlFor={mailboxId}>
            {t("takeover.intoFolder")}
            <select
              id={mailboxId}
              value={target ?? ""}
              onChange={(e) => {
                setMailbox(Number(e.target.value));
              }}
              className="h-8 rounded-control border border-border bg-surface-raised px-2 text-13 text-primary"
              data-testid="takeover-folder-mailbox"
            >
              {mailboxes.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.path}
                </option>
              ))}
            </select>
          </label>
          <Button
            variant="secondary"
            disabled={target === null || start.isPending || start.isSuccess}
            onClick={() => {
              if (target !== null) start.mutate(target);
            }}
            data-testid="takeover-folder-import"
          >
            {t("takeover.importFolder")}
          </Button>
        </div>
      )}
      {start.isSuccess && (
        <p role="status" className="text-12 text-secondary">
          {t("takeover.folderStarted")}
        </p>
      )}
      {start.isError && <ErrorNotice context={t("takeover.folderFailed")} error={start.error} />}
    </li>
  );
}

/** The preview of an opened profile. */
function Preview({ preview, onBack }: { preview: TakeoverPreview; onBack: () => void }) {
  const { t } = useTranslation();
  const accounts = useAccounts().data ?? [];
  const [settingUp, setSettingUp] = useState<TakeoverAccount | null>(null);
  const [added, setAdded] = useState<{ email: string; applied: TakeoverApplied } | null>(null);
  const [applyError, setApplyError] = useState<CoreError | null>(null);

  if (settingUp) {
    return (
      <section className="flex max-w-(--ui-content-max-width) flex-col gap-4" data-testid="takeover-setup">
        <div>
          <Button
            variant="quiet"
            onClick={() => {
              setSettingUp(null);
            }}
          >
            <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("takeover.backToPreview")}
          </Button>
        </div>
        <h2 className="text-20 font-semibold text-primary">{t("takeover.setUpTitle", { email: settingUp.email })}</h2>
        <Notice tone="info" title={t("takeover.passwordTitle")}>
          <p>{t("takeover.passwordBody")}</p>
        </Notice>
        <AddAccount
          prefill={{ email: settingUp.email, displayName: settingUp.displayName, imap: settingUp.imap, smtp: settingUp.smtp }}
          embedded={{
            onAdded: (account) => {
              const email = settingUp.email;
              setSettingUp(null);
              takeoverIpc.applyAccount(preview.token, settingUp.index, account.id).then(
                (applied) => {
                  setAdded({ email, applied });
                },
                (e: unknown) => {
                  setApplyError(toCoreError(e));
                },
              );
            },
          }}
        />
      </section>
    );
  }

  return (
    <section className="flex max-w-(--ui-content-max-width) flex-col gap-6" data-testid="takeover-preview">
      <div>
        <Button variant="quiet" onClick={onBack}>
          <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("takeover.backToProfiles")}
        </Button>
      </div>
      <div className="flex flex-col gap-1">
        <h2 className="text-20 font-semibold text-primary">{t("takeover.previewTitle", { name: preview.profileName })}</h2>
        <p className="text-13 text-secondary">{t("takeover.previewBody")}</p>
      </div>
      {added && (
        <Notice tone="success" role="status" title={t("takeover.addedTitle", { email: added.email })} data-testid="takeover-added">
          <p>{t("takeover.addedBody")}</p>
          <p>{t("takeover.applied", { identities: added.applied.identities, signatures: added.applied.signatures })}</p>
        </Notice>
      )}
      {applyError && <ErrorNotice context={t("takeover.applyFailed")} error={applyError} />}
      <div className="flex flex-col gap-2">
        <h3 className="text-14 font-semibold text-primary">{t("takeover.accounts")}</h3>
        {preview.accounts.length === 0 ? (
          <p className="text-13 text-secondary">{t("takeover.noAccounts")}</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {preview.accounts.map((account) => (
              <AccountCard
                key={account.index}
                preview={preview}
                account={account}
                setUp={accounts.find((a) => a.email.toLowerCase() === account.email.toLowerCase())}
                onSetUp={() => {
                  setSettingUp(account);
                }}
              />
            ))}
          </ul>
        )}
      </div>
      <div className="flex flex-col gap-2">
        <h3 className="text-14 font-semibold text-primary">{t("takeover.folders")}</h3>
        {preview.folders.length === 0 ? (
          <p className="text-13 text-secondary">{t("takeover.noFolders")}</p>
        ) : (
          <>
            <p className="text-13 text-secondary">{t("takeover.foldersBody")}</p>
            <ul className="flex flex-col gap-2">
              {preview.folders.map((folder) => (
                <FolderRow key={folder.index} preview={preview} folder={folder} accounts={accounts} />
              ))}
            </ul>
          </>
        )}
      </div>
    </section>
  );
}

/**
 * Stage 19 (decision 0057): Settings → Accounts → "Import from Thunderbird". Lists the profiles
 * Thunderbird keeps at its usual place (or a folder the user chooses), shows what a profile
 * holds, and takes over what the user picks: accounts through the normal setup (the password is
 * typed there; Thunderbird's are never read), identities and signatures afterwards, local folders
 * through the outbox into a folder of an account that may write.
 */
export function ThunderbirdTakeover() {
  const { t } = useTranslation();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const profiles = useQuery({ queryKey: ["thunderbirdProfiles"], queryFn: takeoverIpc.profiles });
  const [preview, setPreview] = useState<TakeoverPreview | null>(null);
  const open = useMutation({ mutationFn: takeoverIpc.open, onSuccess: setPreview });
  const choose = useMutation({
    mutationFn: takeoverIpc.choose,
    onSuccess: (outcome) => {
      if (outcome.kind === "opened") setPreview(outcome.preview);
    },
  });

  if (preview) {
    return (
      <Preview
        key={preview.token}
        preview={preview}
        onBack={() => {
          setPreview(null);
        }}
      />
    );
  }
  const list = profiles.data ?? [];
  return (
    <section className="flex max-w-(--ui-content-max-width) flex-col gap-6" data-testid="takeover">
      <div>
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
      <div className="flex flex-col gap-1">
        <h2 className="text-20 font-semibold text-primary">{t("takeover.title")}</h2>
        <p className="text-13 text-secondary">{t("takeover.intro")}</p>
      </div>
      <Notice tone="info" title={t("takeover.noPasswordsTitle")}>
        <p className="flex items-start gap-2">
          <KeyRound aria-hidden className="mt-[calc((var(--ui-line-height-14)-var(--ui-icon-small))/2)] size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
          {t("takeover.noPasswordsBody")}
        </p>
      </Notice>
      {profiles.isError && <ErrorNotice context={t("takeover.profilesFailed")} error={profiles.error} />}
      {profiles.isSuccess && list.length === 0 && <p className="text-13 text-secondary">{t("takeover.notFound")}</p>}
      {list.length > 0 && (
        <ul aria-label={t("takeover.profiles")} className="flex flex-col divide-y divide-border-subtle overflow-hidden rounded-card border border-border-subtle bg-surface-raised shadow-card">
          {list.map((profile) => (
            <li key={profile.index} className="flex items-center justify-between gap-2 px-4 py-3" data-testid="takeover-profile">
              <span className="min-w-0 truncate text-14 text-primary">
                {profile.name}
                {profile.isDefault && <span className="ms-2 text-12 text-secondary">{t("takeover.default")}</span>}
              </span>
              <Button
                variant="secondary"
                disabled={open.isPending}
                onClick={() => {
                  open.mutate(profile.index);
                }}
                data-testid="takeover-profile-open"
              >
                {t("takeover.openProfile")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div>
        <Button
          variant="quiet"
          disabled={choose.isPending}
          onClick={() => {
            choose.mutate();
          }}
          data-testid="takeover-choose"
        >
          <FolderOpen aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("takeover.choose")}
        </Button>
      </div>
      {open.isError && <ErrorNotice context={t("takeover.openFailed")} error={open.error} />}
      {choose.isError && <ErrorNotice context={t("takeover.openFailed")} error={choose.error} />}
    </section>
  );
}
