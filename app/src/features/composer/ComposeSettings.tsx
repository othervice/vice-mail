import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import {
  DEFAULT_SEND_DELAY_SECS,
  MAX_SEND_DELAY_SECS,
  type Account,
  type QuotePosition,
  type SentCopyMode,
} from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
// Stage 10: the signature pool, the defaults per identity and the templates.
import { SignatureDefaults } from "../snippets/SignatureDefaults";
import { SignatureSettings } from "../snippets/SignatureSettings";
import { TemplateSettings } from "../snippets/TemplateSettings";
import {
  useComposeIdentities,
  useComposeSettings,
  useSendSettings,
  useSetComposeSettings,
  useSetSentCopyMode,
} from "./queries";

/** Choices for the undo delay, within 0..`MAX_SEND_DELAY_SECS`. */
export const SEND_DELAY_CHOICES = [0, 5, 10, 15, 20, 30].filter((s) => s <= MAX_SEND_DELAY_SECS);

const SENT_COPY_MODES: readonly SentCopyMode[] = ["auto", "app", "server"];

/** Stage 8 (decision 0025): where a new reply or forward puts the answer; "above" is the core's default. */
const QUOTE_POSITIONS: readonly QuotePosition[] = ["above", "below"];

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
          <p className="text-13 text-secondary">{t("snippets.defaults.hint")}</p>
          {identities.data.map((identity) => (
            <SignatureDefaults key={identity.id} identity={identity} />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Settings → Writing: the undo delay, the signature pool, per account who files sent mail and the
 * default signatures of each address, and the templates (stage 10).
 */
export function ComposeSettings() {
  const { t } = useTranslation();
  const delayId = useId();
  const quoteId = useId();
  const quoteHintId = useId();
  const settings = useComposeSettings();
  const save = useSetComposeSettings();
  const accounts = useAccounts();
  const delay = settings.data?.sendDelaySeconds ?? DEFAULT_SEND_DELAY_SECS;
  // Both fields go back on every save: a missing quote position would reset it to the default.
  const quotePosition: QuotePosition = settings.data?.defaultQuotePosition ?? "above";
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
            save.mutate({ sendDelaySeconds: Number(e.target.value), defaultQuotePosition: quotePosition });
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
      </div>
      <div className="flex flex-col gap-1">
        <label htmlFor={quoteId} className="text-13 font-medium text-primary">
          {t("writingDefaults.quotePosition.label")}
        </label>
        <select
          id={quoteId}
          value={quotePosition}
          disabled={!settings.data}
          aria-describedby={quoteHintId}
          onChange={(e) => {
            const position = QUOTE_POSITIONS.find((p) => p === e.target.value);
            if (position) save.mutate({ sendDelaySeconds: delay, defaultQuotePosition: position });
          }}
          className="h-8 w-fit rounded-control border border-border-strong bg-surface px-2 text-13 text-primary"
          data-testid="default-quote-position"
        >
          {QUOTE_POSITIONS.map((position) => (
            <option key={position} value={position}>
              {t(`writingDefaults.quotePosition.${position}`)}
            </option>
          ))}
        </select>
        <p id={quoteHintId} className="text-12 text-secondary">
          {t("writingDefaults.quotePosition.hint")}
        </p>
        {save.isError && <ErrorNotice context={t("composer.settings.saveFailed")} error={save.error} />}
      </div>
      <SignatureSettings />
      {(accounts.data ?? []).map((account) => (
        <AccountComposeSettings key={account.id} account={account} />
      ))}
      <TemplateSettings />
    </SettingsSection>
  );
}
