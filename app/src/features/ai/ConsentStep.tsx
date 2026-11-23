import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { AiFunction, AiProfileInfo, CoreError } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { InlineConfirm } from "../mcp/InlineConfirm";

interface ConsentStepProps {
  /** The function about to run, or null for the general explanation (in the settings). */
  aiFunction: AiFunction | null;
  /** Null when the core asked for consent but the profile is not known here (settings changed). */
  profile: AiProfileInfo | null;
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
export function ConsentStep({ aiFunction, profile, onAgree, onCancel, busy = false, error = null }: ConsentStepProps) {
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
      <p>{t("ai.consent.size")}</p>
      <p>{t("ai.consent.accounts")}</p>
      <p>{t("ai.consent.provider")}</p>
      <p>{t("ai.consent.withdraw")}</p>
    </InlineConfirm>
  );
}
