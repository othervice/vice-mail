import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { AiFunction, AiProfileInfo, AiTransferInput, CoreError } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { InlineConfirm } from "../mcp/InlineConfirm";
import { LocalityBadge, TransferNotes, transferText } from "./AiTransfer";
import { useAiTransferPreview } from "./queries";

/**
 * Decision 0028: the exact size of what the request would send, from the core's dry run
 * (mails, bytes, accounts, host, model, what was shortened). Nothing is sent for it.
 */
function ExactTransfer({ input }: { input: AiTransferInput }) {
  const { t, i18n } = useTranslation();
  const preview = useAiTransferPreview(input);
  if (preview.isPending) {
    return (
      <p role="status" className="text-secondary" data-testid="ai-consent-measuring">
        {t("ai.consent.measuring")}
      </p>
    );
  }
  if (preview.isError) return <ErrorNotice context={t("ai.consent.exactFailed")} error={preview.error} data-testid="ai-consent-exact-error" />;
  const transfer = preview.data;
  return (
    <div className="flex flex-col gap-1" data-testid="ai-consent-exact">
      <p className="flex flex-wrap items-center gap-2 font-medium text-primary">
        <span>{t("ai.consent.exact", { what: transferText(transfer, t, i18n.language) })}</span>
        <LocalityBadge locality={transfer.locality} />
      </p>
      <TransferNotes transfer={transfer} />
      {input.function === "search" && <p className="text-secondary">{t("ai.consent.searchFollows")}</p>}
    </div>
  );
}

interface ConsentStepProps {
  /** The function about to run, or null for the general explanation (in the settings). */
  aiFunction: AiFunction | null;
  /** Null when the core asked for consent but the profile is not known here (settings changed). */
  profile: AiProfileInfo | null;
  /** What the request would get; the step then shows the exact size from the core's dry run. */
  input?: AiTransferInput | null;
  onAgree: () => void;
  onCancel: () => void;
  busy?: boolean;
  error?: CoreError | null;
}

/**
 * Before the first request to a cloud provider: what is sent (which mails or text, how big it can
 * get, without attachments), to whom (the host and model of the profile), and what the app cannot
 * control. Nothing has been sent when this shows; only "Agree" records the consent.
 */
export function ConsentStep({ aiFunction, profile, input = null, onAgree, onCancel, busy = false, error = null }: ConsentStepProps) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  if (!profile) {
    return (
      <div role="group" aria-label={t("ai.consent.unknownTitle")} className="flex flex-col gap-2 rounded-card border border-accent p-4" data-testid="ai-consent">
        <p className="font-semibold text-primary">{t("ai.consent.unknownTitle")}</p>
        <p className="text-secondary">{t("ai.consent.unknownBody")}</p>
        <div className="flex flex-wrap gap-2">
          <Button
            onClick={() => {
              showSettings("ai");
            }}
          >
            {t("ai.openSettings")}
          </Button>
          <Button variant="secondary" onClick={onCancel}>
            {t("ai.consent.cancel")}
          </Button>
        </div>
      </div>
    );
  }
  return (
    <InlineConfirm
      tone="accent"
      title={t("ai.consent.title", { name: profile.name })}
      confirmLabel={t("ai.consent.agree")}
      cancelLabel={t("ai.consent.cancel")}
      onConfirm={onAgree}
      onCancel={onCancel}
      busy={busy}
      footer={error && <ErrorNotice context={t("ai.consent.failed")} error={error} />}
      data-testid="ai-consent"
    >
      <p>{t("ai.consent.nothingYet")}</p>
      <p data-testid="ai-consent-what">{t(`ai.consent.what.${aiFunction ?? "general"}`)}</p>
      <p data-testid="ai-consent-to">{t("ai.consent.to", { host: profile.host, name: profile.name, model: profile.model || t("ai.profiles.noModel") })}</p>
      {input && <ExactTransfer input={input} />}
      <p>{t("ai.consent.size")}</p>
      <p>{t("ai.consent.accounts")}</p>
      <p>{t("ai.consent.provider")}</p>
      <p>{t("ai.consent.withdraw")}</p>
    </InlineConfirm>
  );
}
