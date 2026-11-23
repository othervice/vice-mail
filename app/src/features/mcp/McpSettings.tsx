import { Tabs } from "radix-ui";
import { useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ChoiceGroup, type ChoiceOption } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Notice } from "../../components/Notice";
import { SettingsSection } from "../../components/SettingsSection";
import { Switch } from "../../components/Switch";
import { TextField } from "../../components/TextField";
import {
  MAX_MCP_CLIENT_NAME_CHARS,
  MAX_MCP_CLIENTS,
  type Account,
  type McpAccountAccess,
  type McpAccountShare,
  type McpClientInfo,
  type McpSettingsInfo,
  type McpStatus,
} from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
import { ClientConfigs } from "./ClientConfigs";
import { ConnectionTest } from "./ConnectionTest";
import { EmergencyStop } from "./EmergencyStop";
import { formatMcpTime } from "./format";
import { InlineConfirm } from "./InlineConfirm";
import { McpAccessLog } from "./McpAccessLog";
import {
  useCreateMcpClient,
  useMcpSettings,
  useMcpSetup,
  useRevokeMcpClient,
  useSetMcpAccountAccess,
  useSetMcpEnabled,
} from "./queries";

const ACCESS_LEVELS: readonly McpAccountAccess[] = ["hidden", "read", "read_and_drafts"];

function Block({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6">
      <div className="flex flex-col gap-1">
        <h3 className="text-14 font-semibold text-primary">{title}</h3>
        {description && <p className="text-13 text-secondary">{description}</p>}
      </div>
      {children}
    </section>
  );
}

/** The master switch. Turning it on asks first, with the honest explanation of what a cloud client does. */
function MasterSwitch({ settings }: { settings: McpSettingsInfo }) {
  const { t } = useTranslation();
  const setEnabled = useSetMcpEnabled();
  const [asking, setAsking] = useState(false);
  return (
    <div className="flex flex-col gap-3">
      <Switch
        label={t("mcp.enable.label")}
        description={t("mcp.enable.description")}
        checked={settings.enabled}
        disabled={setEnabled.isPending}
        onChange={(next) => {
          setEnabled.reset();
          if (next) {
            setAsking(true);
          } else {
            setAsking(false);
            setEnabled.mutate(false);
          }
        }}
        data-testid="mcp-enabled"
      />
      {asking && !settings.enabled && (
        <InlineConfirm
          tone="accent"
          title={t("mcp.enable.confirmTitle")}
          confirmLabel={t("mcp.enable.confirm")}
          cancelLabel={t("mcp.enable.cancel")}
          busy={setEnabled.isPending}
          onConfirm={() => {
            setEnabled.mutate(true, {
              onSuccess: () => {
                setAsking(false);
              },
            });
          }}
          onCancel={() => {
            setAsking(false);
          }}
          data-testid="mcp-enable-confirm"
        >
          <p>{t("mcp.enable.cloud")}</p>
          <p>{t("mcp.enable.local")}</p>
          <p>{t("mcp.enable.never")}</p>
        </InlineConfirm>
      )}
      {settings.enabled && <p className="text-13 text-secondary">{t("mcp.enable.reminder")}</p>}
      {setEnabled.isError && <ErrorNotice context={t("mcp.enable.failed")} error={setEnabled.error} />}
    </div>
  );
}

function StatusBlock({ status }: { status: McpStatus }) {
  const { t } = useTranslation();
  const { helper } = status;
  return (
    <Block title={t("mcp.status.title")}>
      {status.state === "failed" ? (
        <div role="alert" className="flex flex-col gap-2 rounded-card border border-danger bg-surface p-4">
          <p className="font-semibold text-primary" data-testid="mcp-state">
            {t("mcp.status.failed")}
          </p>
          <p className="text-secondary">{t("mcp.status.failedHint")}</p>
          {status.error && (
            <details className="text-12 text-tertiary">
              <summary className="w-fit cursor-pointer rounded-control">{t("errors.details")}</summary>
              <pre className="mt-2 font-mono break-words whitespace-pre-wrap select-text">{status.error}</pre>
            </details>
          )}
        </div>
      ) : (
        <p className="text-14 text-primary" data-testid="mcp-state">
          {t(`mcp.status.${status.state}`)}
        </p>
      )}
      {helper.unstableLocation && (
        <Notice tone="warning" title={t("mcp.status.unstableTitle")} data-testid="mcp-unstable">
          <p>{t("mcp.status.unstableBody")}</p>
        </Notice>
      )}
      {!helper.found && (
        <Notice tone="warning" title={t("mcp.status.helperMissingTitle")} data-testid="mcp-helper-missing">
          <p>{t("mcp.status.helperMissingBody")}</p>
        </Notice>
      )}
      {(helper.path ?? status.endpoint) && (
        <dl className="flex flex-col gap-1 text-12 text-secondary">
          {helper.path && (
            <div className="flex flex-wrap gap-x-2">
              <dt>{t("mcp.status.helperPath")}</dt>
              <dd className="font-mono break-all select-text">{helper.path}</dd>
            </div>
          )}
          {status.endpoint && (
            <div className="flex flex-wrap gap-x-2">
              <dt>{t("mcp.status.endpoint")}</dt>
              <dd className="font-mono break-all select-text">{status.endpoint}</dd>
            </div>
          )}
        </dl>
      )}
    </Block>
  );
}

function AccountShare({ share, account }: { share: McpAccountShare; account: Account | undefined }) {
  const { t } = useTranslation();
  const setAccess = useSetMcpAccountAccess();
  const options: ChoiceOption<McpAccountAccess>[] = ACCESS_LEVELS.map((value) => ({
    value,
    label: t(`mcp.accounts.access.${value}`),
  }));
  const label = account ? account.displayName || account.email : t("mcp.accounts.unknownAccount");
  return (
    <li className="flex flex-col gap-2" data-testid={`mcp-share-${share.accountId}`}>
      <ChoiceGroup
        label={account?.displayName ? t("mcp.accounts.accountLabel", { name: label, email: account.email }) : label}
        description={t(`mcp.accounts.accessDescription.${share.access}`)}
        value={share.access}
        options={options}
        onChange={(access) => {
          setAccess.mutate({ accountId: share.accountId, access });
        }}
      />
      {share.access === "read_and_drafts" && account && !account.writesEnabled && (
        <p className="text-13 text-secondary" data-testid="mcp-local-drafts">
          {t("mcp.accounts.localDrafts")}
        </p>
      )}
      {setAccess.isError && <ErrorNotice context={t("mcp.accounts.failed")} error={setAccess.error} />}
    </li>
  );
}

function AccountsBlock({ shares }: { shares: readonly McpAccountShare[] }) {
  const { t } = useTranslation();
  const accounts = useAccounts();
  return (
    <Block title={t("mcp.accounts.title")} description={t("mcp.accounts.description")}>
      {shares.length === 0 ? (
        <p className="text-13 text-secondary">{t("mcp.accounts.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-5" aria-label={t("mcp.accounts.title")}>
          {shares.map((share) => (
            <AccountShare key={share.accountId} share={share} account={accounts.data?.find((a) => a.id === share.accountId)} />
          ))}
        </ul>
      )}
    </Block>
  );
}

function ClientRow({ client }: { client: McpClientInfo }) {
  const { t, i18n } = useTranslation();
  const revoke = useRevokeMcpClient();
  const revoked = client.revokedAt !== null;
  return (
    <li className="flex flex-col gap-2 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4" data-testid={`mcp-client-${client.clientId}`}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <p className="flex flex-wrap items-center gap-2">
            <span className="truncate text-14 font-medium text-primary">{client.name}</span>
            {revoked && (
              <span className="rounded-control border border-border-strong px-2 text-12 font-medium text-secondary">
                {t("mcp.clients.revokedBadge")}
              </span>
            )}
          </p>
          <p className="text-12 text-secondary">
            {[
              t("mcp.clients.created", { time: formatMcpTime(client.createdAt, i18n.language) }),
              client.lastUsedAt === null
                ? t("mcp.clients.neverUsed")
                : t("mcp.clients.lastUsed", { time: formatMcpTime(client.lastUsedAt, i18n.language) }),
              ...(client.revokedAt === null ? [] : [t("mcp.clients.revoked", { time: formatMcpTime(client.revokedAt, i18n.language) })]),
            ].join(" · ")}
          </p>
        </div>
        {!revoked && (
          <Button
            variant="secondary"
            disabled={revoke.isPending}
            onClick={() => {
              revoke.mutate(client.clientId);
            }}
            aria-label={t("mcp.clients.revokeLabel", { name: client.name })}
            data-testid="mcp-revoke"
          >
            {t("mcp.clients.revoke")}
          </Button>
        )}
      </div>
      {revoke.isError && <ErrorNotice context={t("mcp.clients.revokeFailed")} error={revoke.error} />}
    </li>
  );
}

/** Adding a client shows its token once, inside the configurations; closing forgets it. */
function AddClient({ activeClients }: { activeClients: number }) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [showErrors, setShowErrors] = useState(false);
  const create = useCreateMcpClient();
  const trimmed = name.trim();
  const nameError = !trimmed
    ? t("mcp.clients.add.nameRequired")
    : [...trimmed].length > MAX_MCP_CLIENT_NAME_CHARS
      ? t("mcp.clients.add.nameTooLong", { max: MAX_MCP_CLIENT_NAME_CHARS })
      : null;
  const full = activeClients >= MAX_MCP_CLIENTS;

  if (create.data) {
    const created = create.data;
    return (
      <div className="flex flex-col gap-4 rounded-card border border-accent p-4" data-testid="mcp-created">
        <Notice tone="warning" title={t("mcp.created.title", { name: created.client.name })}>
          <p>{t("mcp.created.once")}</p>
        </Notice>
        <ClientConfigs configs={created.configs} label={t("mcp.setup.configsLabel")} />
        <ConnectionTest />
        <div>
          <Button
            onClick={() => {
              // Forget the token: the mutation's state is the only place that held it.
              create.reset();
              setName("");
              setShowErrors(false);
            }}
            data-testid="mcp-created-done"
          >
            {t("mcp.created.done")}
          </Button>
        </div>
      </div>
    );
  }

  const submit = (event: FormEvent) => {
    event.preventDefault();
    setShowErrors(true);
    if (nameError || full) return;
    create.mutate(trimmed);
  };
  return (
    <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-3" data-testid="mcp-add-client">
      <TextField
        label={t("mcp.clients.add.name")}
        description={t("mcp.clients.add.nameDescription")}
        value={name}
        onChange={(next) => {
          setName(next);
          create.reset();
        }}
        error={showErrors ? nameError : null}
        data-testid="mcp-client-name"
      />
      {full && <p className="text-13 text-secondary">{t("mcp.clients.add.tooMany", { max: MAX_MCP_CLIENTS })}</p>}
      <div>
        <Button type="submit" disabled={create.isPending || full} data-testid="mcp-add-submit">
          {create.isPending ? t("mcp.clients.add.adding") : t("mcp.clients.add.submit")}
        </Button>
      </div>
      {create.isError && <ErrorNotice context={t("mcp.clients.add.failed")} error={create.error} />}
    </form>
  );
}

function ClientsBlock({ clients }: { clients: readonly McpClientInfo[] }) {
  const { t } = useTranslation();
  const active = clients.filter((client) => client.revokedAt === null).length;
  return (
    <Block title={t("mcp.clients.title")} description={t("mcp.clients.description")}>
      {clients.length === 0 ? (
        <p className="text-13 text-secondary">{t("mcp.clients.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-2" aria-label={t("mcp.clients.title")}>
          {clients.map((client) => (
            <ClientRow key={client.clientId} client={client} />
          ))}
        </ul>
      )}
      <AddClient activeClients={active} />
    </Block>
  );
}

function SetupBlock() {
  const { t } = useTranslation();
  const setup = useMcpSetup();
  return (
    <Block title={t("mcp.setup.title")} description={t("mcp.setup.description")}>
      {setup.isError && (
        <ErrorNotice
          context={t("mcp.setup.loadFailed")}
          error={setup.error}
          onRetry={() => {
            void setup.refetch();
          }}
        />
      )}
      {setup.data && <ClientConfigs configs={setup.data.configs} label={t("mcp.setup.configsLabel")} />}
      <ConnectionTest />
    </Block>
  );
}

function SettingsPage({ settings }: { settings: McpSettingsInfo }) {
  return (
    <div className="flex flex-col gap-6">
      <MasterSwitch settings={settings} />
      <EmergencyStop />
      <StatusBlock status={settings.status} />
      <AccountsBlock shares={settings.accounts} />
      <ClientsBlock clients={settings.clients} />
      <SetupBlock />
    </div>
  );
}

const PAGES = ["settings", "access"] as const;
type Page = (typeof PAGES)[number];

/** Settings → AI over MCP: the switch, shares, clients and setup help, and the access view. */
export function McpSettings() {
  const { t } = useTranslation();
  const settings = useMcpSettings();
  const [page, setPage] = useState<Page>("settings");
  return (
    <SettingsSection title={t("mcp.title")}>
      <p className="text-14 text-secondary">{t("mcp.intro")}</p>
      {settings.isError && (
        <ErrorNotice
          context={t("mcp.loadFailed")}
          error={settings.error}
          onRetry={() => {
            void settings.refetch();
          }}
        />
      )}
      {settings.data && (
        <Tabs.Root
          value={page}
          onValueChange={(next) => {
            const match = PAGES.find((candidate) => candidate === next);
            if (match) setPage(match);
          }}
          className="flex flex-col gap-6"
        >
          <Tabs.List aria-label={t("mcp.pages.label")} className="flex gap-1 border-b border-border">
            {PAGES.map((id) => (
              <Tabs.Trigger
                key={id}
                value={id}
                data-testid={`mcp-page-${id}`}
                className="-mb-px h-10 border-b-2 border-transparent px-3 text-14 text-secondary transition-colors hover:text-primary data-[state=active]:border-accent data-[state=active]:font-medium data-[state=active]:text-primary"
              >
                {t(`mcp.pages.${id}`)}
              </Tabs.Trigger>
            ))}
          </Tabs.List>
          {PAGES.map((id) => (
            <Tabs.Content key={id} value={id}>
              {id === "settings" ? <SettingsPage settings={settings.data} /> : <McpAccessLog />}
            </Tabs.Content>
          ))}
        </Tabs.Root>
      )}
    </SettingsSection>
  );
}
