import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import type { McpConnectionTest } from "../../services/ipc";
import { useTestMcpConnection } from "./queries";

function TestResult({ result }: { result: McpConnectionTest }) {
  const { t } = useTranslation();
  if (result.ok) {
    const tools = result.tools.map((tool) => t(`mcp.tools.${tool}`)).join(", ");
    return (
      <Notice tone="success" role="status" title={t("mcp.test.okTitle")} data-testid="mcp-test-ok">
        <p>{t("mcp.test.okBody", { count: result.sharedAccounts })}</p>
        {tools && <p>{t("mcp.test.tools", { tools })}</p>}
        {result.protocolVersion && (
          <p className="text-12 text-tertiary">
            {t("mcp.test.protocol", { version: result.protocolVersion, ms: result.durationMs })}
          </p>
        )}
      </Notice>
    );
  }
  const technical = [result.problem, result.refusal, result.helperPath, result.detail].filter(Boolean).join("\n");
  return (
    <div role="alert" className="flex flex-col gap-2 rounded-card border border-danger bg-surface p-4" data-testid="mcp-test-problem">
      <p className="font-semibold text-primary">{t("mcp.test.problemTitle")}</p>
      {result.problem && <p className="text-secondary">{t(`mcp.test.problems.${result.problem}`)}</p>}
      {result.refusal && <p className="text-secondary">{t(`mcp.refusals.${result.refusal}`)}</p>}
      {technical && (
        <details className="text-12 text-tertiary">
          <summary className="w-fit cursor-pointer rounded-control">{t("errors.details")}</summary>
          <pre className="mt-2 font-mono break-words whitespace-pre-wrap select-text">{technical}</pre>
        </details>
      )}
    </div>
  );
}

/** "Test connection": the app starts its helper like an AI client would and reports what it saw. */
export function ConnectionTest() {
  const { t } = useTranslation();
  const test = useTestMcpConnection();
  return (
    <div className="flex flex-col gap-3">
      <div>
        <Button
          variant="secondary"
          disabled={test.isPending}
          onClick={() => {
            test.mutate();
          }}
          data-testid="mcp-test"
        >
          {test.isPending ? t("mcp.test.testing") : t("mcp.test.action")}
        </Button>
      </div>
      {test.data && <TestResult result={test.data} />}
      {test.isError && <ErrorNotice context={t("mcp.test.failed")} error={test.error} />}
    </div>
  );
}
