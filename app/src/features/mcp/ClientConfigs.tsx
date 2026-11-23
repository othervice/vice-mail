import { Copy } from "lucide-react";
import { Tabs } from "radix-ui";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import type { McpClientConfig, McpClientKind } from "../../services/ipc";

type CopyState = "idle" | "copied" | "failed";

function ConfigPanel({ config }: { config: McpClientConfig }) {
  const { t } = useTranslation();
  const [copy, setCopy] = useState<CopyState>("idle");
  const client = t(`mcp.setup.kinds.${config.kind}`);
  const copyContent = () => {
    // Only on the user's click. The text may hold the token; nothing else keeps a copy.
    const clipboard = typeof navigator === "undefined" ? undefined : (navigator.clipboard as Clipboard | undefined);
    if (!clipboard) {
      setCopy("failed");
      return;
    }
    clipboard.writeText(config.content).then(
      () => {
        setCopy("copied");
      },
      () => {
        setCopy("failed");
      },
    );
  };
  return (
    <div className="flex flex-col gap-2 pt-3">
      <p className="text-13 text-secondary select-text" data-testid="mcp-config-location">
        {config.location === "terminal" ? t("mcp.setup.terminal") : t("mcp.setup.location", { location: config.location })}
      </p>
      <pre
        className="max-h-64 overflow-auto rounded-control border border-border bg-canvas p-3 font-mono text-12 break-all whitespace-pre-wrap text-primary select-text"
        data-testid={`mcp-config-content-${config.kind}`}
      >
        {config.content}
      </pre>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="secondary"
          onClick={copyContent}
          aria-label={t("mcp.setup.copyLabel", { client })}
          data-testid={`mcp-config-copy-${config.kind}`}
        >
          <Copy aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("mcp.setup.copy")}
        </Button>
        <span role="status" className="text-13 text-secondary">
          {copy === "copied" && t("mcp.setup.copied")}
          {copy === "failed" && <span className="text-danger">{t("mcp.setup.copyFailed")}</span>}
        </span>
      </div>
      <p className="text-12 text-tertiary select-text">{t("mcp.setup.source", { source: config.source })}</p>
    </div>
  );
}

/** One tab per AI client: where the configuration goes, the text to copy, the client's docs. */
export function ClientConfigs({ configs, label }: { configs: readonly McpClientConfig[]; label: string }) {
  const { t } = useTranslation();
  const [kind, setKind] = useState<McpClientKind | undefined>(configs[0]?.kind);
  const current = configs.some((config) => config.kind === kind) ? kind : configs[0]?.kind;
  if (!current) return null;
  return (
    <Tabs.Root
      value={current}
      onValueChange={(next) => {
        const match = configs.find((config) => config.kind === next);
        if (match) setKind(match.kind);
      }}
      className="flex flex-col"
    >
      <Tabs.List aria-label={label} className="inline-flex w-fit flex-wrap gap-1 rounded-card bg-track p-1">
        {configs.map((config) => (
          <Tabs.Trigger
            key={config.kind}
            value={config.kind}
            data-testid={`mcp-config-tab-${config.kind}`}
            className="h-8 rounded-control px-3 text-13 font-medium text-secondary transition-colors hover:bg-surface-raised/60 hover:text-primary data-[state=active]:bg-surface-raised data-[state=active]:text-primary data-[state=active]:shadow-row data-[state=active]:ring-1 data-[state=active]:ring-accent"
          >
            {t(`mcp.setup.kinds.${config.kind}`)}
          </Tabs.Trigger>
        ))}
      </Tabs.List>
      {configs.map((config) => (
        <Tabs.Content key={config.kind} value={config.kind}>
          <ConfigPanel config={config} />
        </Tabs.Content>
      ))}
    </Tabs.Root>
  );
}
