import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import type { AiLocality, AiTransfer } from "../../services/ipc";
import { useAiRequestStore } from "../../stores/aiRequests";
import { formatSize } from "../reading/format";

export const isLocal = (locality: AiLocality) => locality !== "internet";

/** "Local" or "Cloud" next to a provider, as text (never colour alone). */
export function LocalityBadge({ locality }: { locality: AiLocality }) {
  const { t } = useTranslation();
  return (
    <span
      className={cx(
        "inline-flex h-5 items-center rounded-control border px-2 text-12 font-medium",
        isLocal(locality) ? "border-success text-success" : "border-border-strong text-secondary",
      )}
      title={t(`ai.locality.${locality}`)}
      data-testid="ai-locality"
    >
      {isLocal(locality) ? t("ai.locality.local") : t("ai.locality.cloud")}
    </span>
  );
}

/** What one request sends and to whom: "3 mails, 12 KB to api.example.com:443 (model)". */
export function transferText(transfer: AiTransfer, t: TFunction, language: string): string {
  const size = formatSize(transfer.bytes, language);
  return transfer.mails > 0
    ? t("ai.transfer.mails", { count: transfer.mails, size, host: transfer.host, model: transfer.model })
    : t("ai.transfer.text", { size, host: transfer.host, model: transfer.model });
}

/** Shortened, quotes left out, attachment names: what the user should know about the content sent. */
export function TransferNotes({ transfer }: { transfer: AiTransfer }) {
  const { t } = useTranslation();
  const { trimming } = transfer;
  const notes: string[] = [];
  if (trimming.mailsLeftOut > 0) notes.push(t("ai.transfer.leftOut", { count: trimming.mailsLeftOut }));
  if (trimming.mailsShortened > 0) notes.push(t("ai.transfer.cutShort", { count: trimming.mailsShortened }));
  if (trimming.charsRemoved > 0 && trimming.mailsLeftOut === 0 && trimming.mailsShortened === 0) {
    notes.push(t("ai.transfer.charsRemoved"));
  }
  if (transfer.quotesRemoved > 0) notes.push(t("ai.transfer.quotesRemoved", { count: transfer.quotesRemoved }));
  if (transfer.attachmentNames > 0) notes.push(t("ai.transfer.attachmentNames", { count: transfer.attachmentNames }));
  if (notes.length === 0) return null;
  return (
    <ul className="flex flex-col gap-1 text-12 text-secondary" data-testid="ai-transfer-notes">
      {notes.map((note) => (
        <li key={note}>{note}</li>
      ))}
    </ul>
  );
}

/** After a request: what went where, as the core reported it. */
export function TransferSummary({ transfers }: { transfers: readonly AiTransfer[] }) {
  const { t, i18n } = useTranslation();
  if (transfers.length === 0) return null;
  return (
    <div className="flex flex-col gap-1 border-t border-border pt-2 text-12 text-secondary" data-testid="ai-transfer-summary">
      {transfers.map((transfer, index) => (
        <div key={index} className="flex flex-col gap-1">
          <p className="flex flex-wrap items-center gap-2">
            <span>{t("ai.transfer.sent", { what: transferText(transfer, t, i18n.language) })}</span>
            <LocalityBadge locality={transfer.locality} />
          </p>
          <TransferNotes transfer={transfer} />
        </div>
      ))}
    </div>
  );
}

/**
 * While a request runs: what the core is sending to whom (from the `aiRequest` events), the
 * progress of the answer, and "Cancel". Announced politely to screen readers.
 */
export function AiProgress({ requestId, onCancel }: { requestId: string; onCancel: () => void }) {
  const { t, i18n } = useTranslation();
  const progress = useAiRequestStore((state) => state.requests[requestId]);
  const transfer = progress?.transfers[progress.transfers.length - 1];
  const numbers = new Intl.NumberFormat(i18n.language);
  const step =
    progress && progress.transfers.length > 1
      ? t("ai.progress.step", { step: progress.transfers.length })
      : null;
  return (
    <div className="flex flex-col gap-2 rounded-card border border-accent bg-canvas p-3" data-testid="ai-progress">
      <div role="status" aria-live="polite" className="flex flex-col gap-1 text-13">
        {transfer ? (
          <>
            <p className="flex flex-wrap items-center gap-2 text-primary" data-testid="ai-progress-transfer">
              <span>{[step, t("ai.progress.sending", { what: transferText(transfer, t, i18n.language) })].filter(Boolean).join(" ")}</span>
              <LocalityBadge locality={transfer.locality} />
            </p>
            {progress.chars > 0 && (
              <p className="text-secondary">{t("ai.progress.receiving", { chars: numbers.format(progress.chars) })}</p>
            )}
          </>
        ) : (
          <p className="text-secondary">{t("ai.progress.preparing")}</p>
        )}
      </div>
      {transfer && <TransferNotes transfer={transfer} />}
      <div>
        <Button variant="secondary" onClick={onCancel} data-testid="ai-cancel">
          {t("ai.progress.cancel")}
        </Button>
      </div>
    </div>
  );
}
