import { ArrowLeft, Globe } from "lucide-react";
import { useEffect, useId, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { Notice } from "../../../components/Notice";
import { TextField } from "../../../components/TextField";
import type { Account, DiscoveryResult, ServerSettings } from "../../../services/ipc";
import { useUiStore } from "../../../stores/ui";
import {
  useAddAccount,
  useDiscoverOnline,
  useKnownProvider,
  useTestConnection,
} from "../../accounts/queries";
import {
  domainOf,
  emptyServers,
  fromDiscovery,
  isEmail,
  isGmailHost,
  toSettings,
  type ServerDraft,
  type ServerKind,
  type ServersDraft,
} from "./serverDraft";
import { ServerFields } from "./ServerFields";
import { JmapFields, ProtocolChoice } from "./ProtocolChoice";
import { useJmapChoice } from "./jmapChoice";
import { OAuthSetup, oauthProviderFor } from "./OAuthSignIn";

/** Waits until typing pauses before the local provider lookup runs. */
function useDebounced<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(value);
    }, delay);
    return () => {
      window.clearTimeout(timer);
    };
  }, [value, delay]);
  return debounced;
}

/** Where the server settings in the form came from. */
type ServerOrigin = { kind: "user" } | { kind: "online"; source: "srv" | "autoconfig"; domain: string };

function ServerSummary({ settings, label }: { settings: ServerDraft; label: string }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col">
      <dt className="text-12 text-secondary">{label}</dt>
      <dd className="text-13 text-primary select-text">
        {t("settings.accounts.setup.serverSummary", {
          host: settings.host,
          port: settings.port,
          security: t(`settings.accounts.setup.securityOptions.${settings.security}`),
        })}
      </dd>
      <dd className="text-12 text-secondary select-text">
        {t("settings.accounts.setup.usernameValue", { username: settings.username })}
      </dd>
    </div>
  );
}

function knownSuggestion(result: DiscoveryResult | undefined): DiscoveryResult | null {
  return result?.source === "knownProvider" ? result : null;
}

function serverOf(settings: ServerSettings | null | undefined): boolean {
  return settings != null && settings.host.trim() !== "";
}

interface AddAccountProps {
  /**
   * Stage 15 (decision 0044): the form inside the guided first start. The step has the heading
   * and the way on, so the form leaves out its own heading, the way back to the account list and
   * "Cancel", and reports the new account instead of opening the inbox.
   */
  embedded?: { onAdded: (account: Account) => void };
}

/**
 * Account setup as its own view (docs/DESIGN.md: no dialogs for everyday things). Address,
 * name and password first; the core's built-in provider table answers at once and offline.
 * Anything else is found only on request (DNS and the domain's autoconfig file, explained
 * before the click) or entered by hand. Online answers are not DNSSEC-verified, so the found
 * servers are always shown, with a note to check them, before the user adds the account. "Test connection" logs in without storing anything;
 * "Add account" checks the login again in the core and stores the account read-only.
 */
export function AddAccount({ embedded }: AddAccountProps = {}) {
  const { t } = useTranslation();
  const titleId = useId();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const showMailView = useUiStore((state) => state.showMailView);
  const selectAccount = useUiStore((state) => state.selectAccount);

  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  /** Server settings the user edited or an online search found; null follows the provider table. */
  const [manual, setManual] = useState<ServersDraft | null>(null);
  const [origin, setOrigin] = useState<ServerOrigin | null>(null);
  const [editing, setEditing] = useState(false);
  const [attempted, setAttempted] = useState(false);

  const debouncedEmail = useDebounced(email.trim(), 300);
  const known = useKnownProvider(isEmail(debouncedEmail) ? debouncedEmail : null);
  const online = useDiscoverOnline();
  const test = useTestConnection();
  const add = useAddAccount();

  const domain = domainOf(email);
  const suggestion = knownSuggestion(known.data);
  const servers = manual ?? (suggestion ? fromDiscovery(suggestion, email) : emptyServers(email));
  const discovered = manual === null ? suggestion !== null : origin?.kind === "online";
  const showForm = editing || !discovered;
  // Stage 14 (decision 0041): JMAP instead of IMAP and SMTP, chosen in the server settings.
  const jmap = useJmapChoice(email, displayName, online.data);
  const settings = jmap.protocol === "jmap" ? jmap.settings : toSettings(email, displayName, servers);
  const ready = settings !== null && password !== "";
  // Stage 14: Gmail also by its server (Google Workspace domains, servers typed in by hand).
  const providerHint =
    suggestion?.providerHint ?? (online.data?.providerHint || null) ?? (isGmailHost(servers.imap.host) ? "gmail" : null);
  // Stage 14 (decision 0042): sign-in with Microsoft or Google where the address belongs to
  // them. Microsoft accepts no password at all, so its accounts have no password field.
  const oauthProvider = oauthProviderFor(providerHint, servers.imap.host);
  const passwordless = oauthProvider === "microsoft";
  const lookupSettled = isEmail(email) && debouncedEmail === email.trim() && (known.isSuccess || known.isError);
  const lookupDone = lookupSettled && known.isSuccess;
  // Address first: the server part appears once the lookup for it has answered.
  const serversVisible = manual !== null || attempted || lookupSettled;
  const busy = test.isPending || add.isPending;

  /** Any change makes an earlier test result or error meaningless. */
  const changed = () => {
    test.reset();
    add.reset();
  };

  const changeEmail = (next: string) => {
    if (domainOf(next) !== domain && origin?.kind === "online") {
      // Settings found online belong to the old domain.
      setManual(null);
      setOrigin(null);
      setEditing(false);
    }
    online.reset();
    setEmail(next);
    changed();
  };

  const changeServer = (kind: ServerKind, draft: ServerDraft) => {
    setManual({ ...servers, [kind]: draft });
    setOrigin({ kind: "user" });
    changed();
  };

  const searchOnline = () => {
    const address = email.trim();
    online.mutate(address, {
      onSuccess: (result) => {
        if (result.source === "none" || result.source === "knownProvider") return;
        if (!serverOf(result.imap) && !serverOf(result.smtp) && !serverOf(result.jmap)) return;
        setManual(fromDiscovery(result, address));
        setOrigin({ kind: "online", source: result.source, domain: domainOf(address) });
        // Only one of the servers found: the user completes the other in the form.
        setEditing(!serverOf(result.imap) || !serverOf(result.smtp));
        changed();
      },
    });
  };

  const runTest = () => {
    setAttempted(true);
    if (!settings || !password) return;
    add.reset();
    test.mutate({ settings, password });
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    setAttempted(true);
    if (!settings || !password) return;
    test.reset();
    add.mutate(
      { settings, password },
      {
        onSuccess: (account) => {
          setPassword("");
          selectAccount(account.id);
          showMailView("inbox");
          embedded?.onAdded(account);
        },
      },
    );
  };

  let sourceNote: string | null = null;
  if (manual === null && suggestion) sourceNote = t("settings.accounts.setup.known", { domain });
  if (origin?.kind === "online") {
    sourceNote =
      origin.source === "srv"
        ? t("settings.accounts.setup.foundSrv", { domain: origin.domain })
        : t("settings.accounts.setup.foundAutoconfig", { domain: origin.domain });
  }
  const offerOnline = lookupDone && !suggestion && origin?.kind !== "online";
  const nothingFoundOnline = online.isSuccess && !serverOf(online.data.jmap) && (online.data.source === "none" || online.data.source === "knownProvider" || (!serverOf(online.data.imap) && !serverOf(online.data.smtp)));

  const form = (
      <form noValidate onSubmit={submit} className="flex max-w-(--ui-form-max-width) flex-col gap-6" data-testid="add-account">
        <div className="flex flex-col gap-4">
          <TextField
            type="email"
            label={t("settings.accounts.setup.email")}
            value={email}
            onChange={changeEmail}
            autoComplete="email"
            required
            error={attempted && !isEmail(email) ? t("settings.accounts.setup.invalidEmail") : null}
            data-testid="setup-email"
          />
          <TextField
            label={t("settings.accounts.setup.name")}
            description={t("settings.accounts.setup.nameDescription")}
            value={displayName}
            onChange={(next) => {
              setDisplayName(next);
              changed();
            }}
            autoComplete="name"
            data-testid="setup-name"
          />
          {!passwordless && (
            <TextField
              type="password"
              label={t("settings.accounts.setup.password")}
              description={t("settings.accounts.setup.passwordDescription")}
              value={password}
              onChange={(next) => {
                setPassword(next);
                changed();
              }}
              required
              error={attempted && !password ? t("settings.accounts.setup.required") : null}
              data-testid="setup-password"
            />
          )}
        </div>

        {providerHint && (
          <Notice tone="warning" title={t(`settings.accounts.setup.providerHint.${providerHint}.title`)} data-testid="provider-hint">
            <p>{t(`settings.accounts.setup.providerHint.${providerHint}.body`)}</p>
          </Notice>
        )}

        {oauthProvider && serversVisible && (
          <OAuthSetup
            provider={oauthProvider}
            settings={settings}
            onAdded={(account) => {
              selectAccount(account.id);
              showMailView("inbox");
              embedded?.onAdded(account);
            }}
          />
        )}

        {offerOnline && (
          <Notice title={t("settings.accounts.setup.unknown", { domain })} data-testid="online-search">
            <p>{t("settings.accounts.setup.onlineExplanation", { domain })}</p>
            <div>
              <Button variant="secondary" onClick={searchOnline} disabled={online.isPending}>
                <Globe aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
                {online.isPending ? t("settings.accounts.setup.searching") : t("settings.accounts.setup.onlineAction")}
              </Button>
            </div>
            {nothingFoundOnline && <p role="status">{t("settings.accounts.setup.nothingFound")}</p>}
          </Notice>
        )}
        {online.isError && <ErrorNotice context={t("settings.accounts.setup.searchFailed")} error={online.error} />}
        {known.isError && (
          <ErrorNotice
            context={t("settings.accounts.setup.searchFailed")}
            error={known.error}
            onRetry={() => {
              void known.refetch();
            }}
          />
        )}

        {serversVisible && sourceNote && !showForm && (
          <div className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4" data-testid="server-summary">
            <p className="text-14 font-medium text-primary">{sourceNote}</p>
            {origin?.kind === "online" && <p className="text-13 text-secondary">{t("settings.accounts.setup.checkFound")}</p>}
            <dl className="flex flex-col gap-2">
              <ServerSummary label={t("settings.accounts.setup.incoming")} settings={servers.imap} />
              <ServerSummary label={t("settings.accounts.setup.outgoing")} settings={servers.smtp} />
            </dl>
            <div>
              <Button
                variant="quiet"
                onClick={() => {
                  setManual(servers);
                  setEditing(true);
                }}
              >
                {t("settings.accounts.setup.showServers")}
              </Button>
            </div>
          </div>
        )}
        {serversVisible && showForm && (
          <div className="flex flex-col gap-4">
            {sourceNote && <p className="text-14 font-medium text-primary">{sourceNote}</p>}
            {origin?.kind === "online" && <p className="text-13 text-secondary">{t("settings.accounts.setup.checkFound")}</p>}
            <ProtocolChoice
              value={jmap.protocol}
              onChange={(protocol) => {
                jmap.setProtocol(protocol);
                changed();
              }}
            />
            {jmap.protocol === "jmap" ? (
              <JmapFields
                draft={jmap.draft}
                onChange={(draft) => {
                  jmap.setDraft(draft);
                  changed();
                }}
                showErrors={attempted}
              />
            ) : (
              <>
                <ServerFields
                  kind="imap"
                  legend={t("settings.accounts.setup.incoming")}
                  draft={servers.imap}
                  onChange={(draft) => {
                    changeServer("imap", draft);
                  }}
                  showErrors={attempted}
                />
                <ServerFields
                  kind="smtp"
                  legend={t("settings.accounts.setup.outgoing")}
                  note={t("settings.accounts.setup.outgoingNote")}
                  draft={servers.smtp}
                  onChange={(draft) => {
                    changeServer("smtp", draft);
                  }}
                  showErrors={attempted}
                />
              </>
            )}
          </div>
        )}

        {serversVisible && jmap.found && jmap.protocol === "imap" && (
          <p className="text-13 text-secondary" data-testid="jmap-found">
            {t("settings.accounts.setup.jmap.found", { domain, host: jmap.found.host })}
          </p>
        )}

        <Notice title={t("settings.accounts.setup.readOnlyTitle")} data-testid="read-only-note">
          <p>{t("settings.accounts.setup.readOnlyBody")}</p>
        </Notice>

        {test.isSuccess && (
          <Notice tone="success" role="status" title={t("settings.accounts.setup.testOk")} data-testid="test-result">
            <p>
              {test.data.capabilities.jmap
                ? test.data.capabilities.idle
                  ? t("settings.accounts.setup.jmap.testPush")
                  : t("settings.accounts.setup.jmap.testNoPush")
                : test.data.capabilities.idle
                  ? t("settings.accounts.setup.testIdle")
                  : t("settings.accounts.setup.testNoIdle")}
            </p>
            <p>{t("settings.accounts.setup.outgoingNote")}</p>
          </Notice>
        )}
        {test.isError && (
          <ErrorNotice context={t("settings.accounts.setup.testFailed")} error={test.error} data-testid="setup-error" />
        )}
        {add.isError && (
          <ErrorNotice context={t("settings.accounts.setup.addFailed")} error={add.error} data-testid="setup-error" />
        )}

        <div className="flex flex-wrap gap-2">
          {!passwordless && (
            <>
              <Button type="submit" disabled={busy || (attempted && !ready)} data-testid="setup-submit">
                {add.isPending ? t("settings.accounts.setup.adding") : t("settings.accounts.setup.submit")}
              </Button>
              <Button variant="secondary" onClick={runTest} disabled={busy || (attempted && !ready)} data-testid="setup-test">
                {test.isPending ? t("settings.accounts.setup.testing") : t("settings.accounts.setup.test")}
              </Button>
            </>
          )}
          {!embedded && (
            <Button
              variant="quiet"
              onClick={() => {
                showAccountsPage({ kind: "list" });
              }}
            >
              {t("settings.accounts.setup.cancel")}
            </Button>
          )}
        </div>
      </form>
  );
  if (embedded) return form;

  return (
    <section aria-labelledby={titleId} className="flex max-w-(--ui-content-max-width) flex-col gap-6">
      <div>
        <Button
          variant="quiet"
          onClick={() => {
            showAccountsPage({ kind: "list" });
          }}
        >
          <ArrowLeft aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("settings.accounts.back")}
        </Button>
      </div>
      <div className="flex flex-col gap-2">
        <h2 id={titleId} className="text-20 font-semibold text-primary">
          {t("settings.accounts.setup.title")}
        </h2>
        <p className="text-14 text-secondary">{t("settings.accounts.setup.intro")}</p>
      </div>
      {form}
    </section>
  );
}
