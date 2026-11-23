import { ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { AI_LIMITS, type AiRequestEntry } from "../../services/ipc";
import { formatMcpTime } from "../mcp/format";
import { formatSize } from "../reading/format";
import { LocalityBadge } from "./AiTransfer";
import { useAiRequestLog } from "./queries";

const OUTCOME_CLASS: Record<AiRequestEntry["outcome"], string> = {
  ok: "text-success",
  cancelled: "text-secondary",
  failed: "text-danger",
};

function RequestRow({ entry }: { entry: AiRequestEntry }) {
  const { t, i18n } = useTranslation();
  const seconds = new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 1 }).format(entry.durationMs / 1000);
  return (
    <tr className="border-b border-border align-top" data-testid={`ai-request-${String(entry.id)}`}>
      <td className="py-2 pe-4 text-12 whitespace-nowrap text-secondary tabular-nums">
        <time dateTime={new Date(entry.at).toISOString()}>{formatMcpTime(entry.at, i18n.language)}</time>
      </td>
      <td className="py-2 pe-4 text-13 text-primary">{t(`ai.functions.${entry.function}`)}</td>
      <td className="py-2 pe-4 text-13 text-primary">
        <span className="flex flex-wrap items-center gap-2">
          {entry.profileName}
          <LocalityBadge locality={entry.locality} />
        </span>
        <span className="block text-12 text-secondary">{[entry.host, entry.model].filter(Boolean).join(" · ")}</span>
      </td>
      <td className="py-2 pe-4 text-13 text-secondary tabular-nums">
        {t("ai.log.size", { count: entry.mails, size: formatSize(entry.bytes, i18n.language) })}
      </td>
      <td className={cx("py-2 text-13 font-medium", OUTCOME_CLASS[entry.outcome])}>
        {t(`ai.log.outcomes.${entry.outcome}`)}
        {entry.error && <span className="block text-12 font-normal text-secondary">{t(`errors.codes.${entry.error}.title`)}</span>}
        <span className="block text-12 font-normal text-secondary tabular-nums">{t("ai.log.duration", { seconds })}</span>
      </td>
    </tr>
  );
}

/** Every request to a provider, newest first. Metadata only: never the question, mail text, answer or key. */
export function AiRequestLog() {
  const { t } = useTranslation();
  const log = useAiRequestLog();
  const entries = log.data?.pages.flatMap((page) => page.entries) ?? [];
  return (
    <div className="flex flex-col gap-4" data-testid="ai-request-log">
      <p className="text-13 text-secondary">
        {t("ai.log.description", { days: AI_LIMITS.logRetentionDays, max: AI_LIMITS.logMaxEntries })}
      </p>
      {log.isError && (
        <ErrorNotice
          context={t("ai.log.loadFailed")}
          error={log.error}
          onRetry={() => {
            void log.refetch();
          }}
        />
      )}
      {log.isSuccess && entries.length === 0 && (
        <div className="flex rounded-card border border-border bg-canvas">
          <EmptyState icon={ShieldCheck} title={t("ai.log.empty.title")} body={t("ai.log.empty.body")} />
        </div>
      )}
      {entries.length > 0 && (
        <table className="w-full border-collapse text-left" data-testid="ai-request-table">
          <caption className="sr-only">{t("ai.log.caption")}</caption>
          <thead>
            <tr className="border-b border-border-strong text-12 text-secondary">
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("ai.log.columns.time")}
              </th>
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("ai.log.columns.function")}
              </th>
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("ai.log.columns.provider")}
              </th>
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("ai.log.columns.sent")}
              </th>
              <th scope="col" className="py-2 font-medium">
                {t("ai.log.columns.outcome")}
              </th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <RequestRow key={entry.id} entry={entry} />
            ))}
          </tbody>
        </table>
      )}
      {log.hasNextPage && (
        <div>
          <Button
            variant="secondary"
            disabled={log.isFetchingNextPage}
            onClick={() => {
              void log.fetchNextPage();
            }}
            data-testid="ai-log-more"
          >
            {t("ai.log.loadMore")}
          </Button>
        </div>
      )}
    </div>
  );
}
