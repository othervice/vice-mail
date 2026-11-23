import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import {
  DEFAULT_SEND_DELAY_SECS,
  MAX_SEND_DELAY_SECS,
  type Account,
  type ComposeIdentity,
  type SentCopyMode,
} from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
import {
  useComposeIdentities,
  useComposeSettings,
  useSendSettings,
  useSetComposeSettings,
  useSetIdentitySignature,
  useSetSentCopyMode,
} from "./queries";

/** Choices for the undo delay, within 0..`MAX_SEND_DELAY_SECS`. */
export const SEND_DELAY_CHOICES = [0, 5, 10, 15, 20, 30].filter((s) => s <= MAX_SEND_DELAY_SECS);

const SENT_COPY_MODES: readonly SentCopyMode[] = ["auto", "app", "server"];

function SignatureEditor({ identity }: { identity: ComposeIdentity }) {
  const { t } = useTranslation();
  const id = useId();
  const [value, setValue] = useState(identity.signature ?? "");
  const save = useSetIdentitySignature();
  const changed = value.trim() !== (identity.signature ?? "").trim();
  return (
    <div className="flex flex-col gap-2" data-testid="signature-editor">
      <label htmlFor={id} className="text-13 font-medium text-primary">
        {identity.name ? t("composer.identity", { name: identity.name, email: identity.email }) : identity.email}
      </label>
      <textarea
        id={id}
        value={value}
        rows={3}
        onChange={(e) => {
          setValue(e.target.value);
        }}
        className="resize-y rounded-control border border-border-strong bg-surface px-3 py-2 font-sans text-13 text-primary"
        data-testid="signature-input"
      />
      <div className="flex items-center gap-3">
        <Button
          variant="secondary"
          disabled={!changed || save.isPending}
          onClick={() => {
            save.mutate({ identity, signature: value.trim() === "" ? null : value });
          }}
          data-testid="signature-save"
        >
          {t("composer.settings.signatureSave")}
        </Button>
        {save.isSuccess && !changed && (
          <p role="status" className="text-12 text-secondary">
            {t("composer.settings.signatureSaved")}
          </p>
        )}
      </div>
      {save.isError && <ErrorNotice context={t("composer.settings.signatureFailed")} error={save.error} />}
    </div>
  );
}

function AccountComposeSettings({ account }: { account: Account }) {
  const { t } = useTranslation();
  const modeId = useId();
  const settings = useSendSettings(account.id);
  const setMode = useSetSentCopyMode();
  const identities = useComposeIdentities(account.id);
  return (
    <div className="flex flex-col gap-4 rounded-card border border-border p-4" data-testid="account-compose-settings">
      <h3 className="text-14 font-semibold text-primary">{account.displayName ? t("composer.identity", { name: account.displayName, email: account.email }) : account.email}</h3>
      {settings.isError && <ErrorNotice context={t("composer.settings.loadFailed")} error={settings.error} onRetry={() => void settings.refetch()} />}
      {settings.data && (
        <div className="flex flex-col gap-1">
          <label htmlFor={modeId} className="text-13 font-medium text-primary">
            {t("composer.settings.sentCopy")}
          </label>
          <select
            id={modeId}
            value={settings.data.sentCopy}
            onChange={(e) => {
              const mode = SENT_COPY_MODES.find((m) => m === e.target.value);
              if (mode) setMode.mutate({ accountId: account.id, mode });
            }}
            className="h-8 w-fit rounded-control border border-border-strong bg-surface px-2 text-13 text-primary"
            data-testid="sent-copy-mode"
          >
            {SENT_COPY_MODES.map((mode) => (
              <option key={mode} value={mode}>
                {t(`composer.settings.sentCopyMode.${mode}`)}
              </option>
            ))}
          </select>
          <p className="text-12 text-secondary">
            {settings.data.serverSavesSent ? t("composer.settings.serverSaves") : t("composer.settings.appSaves")}
          </p>
          {setMode.isError && <ErrorNotice context={t("composer.settings.saveFailed")} error={setMode.error} />}
        </div>
      )}
      {identities.isError && (
        <ErrorNotice context={t("composer.settings.loadFailed")} error={identities.error} onRetry={() => void identities.refetch()} />
      )}
      {identities.data && identities.data.length > 0 && (
        <div className="flex flex-col gap-3">
          <p className="text-13 text-secondary">{t("composer.settings.signatureHint")}</p>
          {identities.data.map((identity) => (
            <SignatureEditor key={identity.id} identity={identity} />
          ))}
        </div>
      )}
    </div>
  );
}

/** Settings → General → Writing: the undo delay, and per account who files sent mail and the signatures. */
export function ComposeSettings() {
  const { t } = useTranslation();
  const delayId = useId();
  const settings = useComposeSettings();
  const save = useSetComposeSettings();
  const accounts = useAccounts();
  const delay = settings.data?.sendDelaySeconds ?? DEFAULT_SEND_DELAY_SECS;
  const choices = SEND_DELAY_CHOICES.includes(delay) ? SEND_DELAY_CHOICES : [...SEND_DELAY_CHOICES, delay].sort((a, b) => a - b);
  return (
    <SettingsSection title={t("composer.settings.title")}>
      {settings.isError && <ErrorNotice context={t("composer.settings.loadFailed")} error={settings.error} onRetry={() => void settings.refetch()} />}
      <div className="flex flex-col gap-1">
        <label htmlFor={delayId} className="text-13 font-medium text-primary">
          {t("composer.settings.delay")}
        </label>
        <select
          id={delayId}
          value={delay}
          disabled={!settings.data}
          onChange={(e) => {
            save.mutate({ sendDelaySeconds: Number(e.target.value) });
          }}
          className="h-8 w-fit rounded-control border border-border-strong bg-surface px-2 text-13 text-primary"
          data-testid="send-delay"
        >
          {choices.map((seconds) => (
            <option key={seconds} value={seconds}>
              {seconds === 0 ? t("composer.settings.delayOff") : t("composer.settings.delaySeconds", { count: seconds })}
            </option>
          ))}
        </select>
        <p className="text-12 text-secondary">{t("composer.settings.delayHint")}</p>
        {save.isError && <ErrorNotice context={t("composer.settings.saveFailed")} error={save.error} />}
      </div>
      {(accounts.data ?? []).map((account) => (
        <AccountComposeSettings key={account.id} account={account} />
      ))}
    </SettingsSection>
  );
}
