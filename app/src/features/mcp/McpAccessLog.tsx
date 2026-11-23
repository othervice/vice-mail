import { ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { MCP_ACCESS_MAX_ENTRIES, MCP_ACCESS_RETENTION_DAYS, type McpAccessEntry } from "../../services/ipc";
import { EmergencyStop } from "./EmergencyStop";
import { formatMcpTime } from "./format";
import { useMcpAccess, useMcpSettings } from "./queries";

const OUTCOME_CLASS: Record<McpAccessEntry["outcome"], string> = {
  ok: "text-success",
  error: "text-danger",
  refused: "text-warning",
};

function AccessRow({ entry }: { entry: McpAccessEntry }) {
  const { t, i18n } = useTranslation();
  const what = entry.tool
    ? t(`mcp.tools.${entry.tool}`)
    : t("mcp.access.connectionRefused", { reason: entry.refusal ? t(`mcp.refusals.${entry.refusal}`) : t("mcp.access.unknownReason") });
  return (
    <tr className="border-b border-border align-top" data-testid={`mcp-access-${String(entry.id)}`}>
      <td className="py-2 pe-4 text-12 whitespace-nowrap text-secondary tabular-nums">
        <time dateTime={new Date(entry.at).toISOString()}>{formatMcpTime(entry.at, i18n.language)}</time>
      </td>
      <td className="py-2 pe-4 text-13 text-primary">{entry.clientName ?? t("mcp.access.unknownClient")}</td>
      <td className="py-2 pe-4 text-13 text-primary">
        {what}
        {entry.tool && entry.refusal && (
          <span className="block text-12 text-secondary">{t(`mcp.refusals.${entry.refusal}`)}</span>
        )}
      </td>
      <td className="py-2 pe-4 text-13 text-secondary tabular-nums">{t("mcp.access.messages", { count: entry.messages })}</td>
      <td className={cx("py-2 text-13 font-medium", OUTCOME_CLASS[entry.outcome])}>{t(`mcp.access.outcomes.${entry.outcome}`)}</td>
    </tr>
  );
}

/** "Access": every call of an AI client, newest first, live through `mcpAccess`. Metadata only. */
export function McpAccessLog() {
  const { t } = useTranslation();
  const settings = useMcpSettings();
  const access = useMcpAccess();
  const entries = access.data?.pages.flatMap((page) => page.entries) ?? [];
  return (
    <div className="flex flex-col gap-4">
      <p className="text-13 text-secondary">
        {t("mcp.access.description", { days: MCP_ACCESS_RETENTION_DAYS, max: MCP_ACCESS_MAX_ENTRIES })}
      </p>
      {settings.data && !settings.data.enabled && (
        <p className="text-13 text-secondary" data-testid="mcp-access-off">
          {t("mcp.access.off")}
        </p>
      )}
      <EmergencyStop />
      {access.isError && (
        <ErrorNotice
          context={t("mcp.access.loadFailed")}
          error={access.error}
          onRetry={() => {
            void access.refetch();
          }}
        />
      )}
      {access.isSuccess && entries.length === 0 && (
        <div className="flex rounded-card border border-border-subtle bg-surface-raised shadow-card">
          <EmptyState icon={ShieldCheck} title={t("mcp.access.empty.title")} body={t("mcp.access.empty.body")} />
        </div>
      )}
      {entries.length > 0 && (
        <table className="w-full border-collapse text-left" data-testid="mcp-access-table">
          <caption className="sr-only">{t("mcp.access.caption")}</caption>
          <thead>
            <tr className="border-b border-border-strong text-12 text-secondary">
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("mcp.access.columns.time")}
              </th>
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("mcp.access.columns.client")}
              </th>
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("mcp.access.columns.tool")}
              </th>
              <th scope="col" className="py-2 pe-4 font-medium">
                {t("mcp.access.columns.messages")}
              </th>
              <th scope="col" className="py-2 font-medium">
                {t("mcp.access.columns.outcome")}
              </th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <AccessRow key={entry.id} entry={entry} />
            ))}
          </tbody>
        </table>
      )}
      {access.hasNextPage && (
        <div>
          <Button
            variant="secondary"
            disabled={access.isFetchingNextPage}
            onClick={() => {
              void access.fetchNextPage();
            }}
            data-testid="mcp-access-more"
          >
            {t("mcp.access.loadMore")}
          </Button>
        </div>
      )}
    </div>
  );
}
