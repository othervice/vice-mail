import type { AccountSettings, CredentialKind, DiscoveryResult, Security, ServerSettings } from "../../../services/ipc";

/** A server as the form edits it: the port is text until the form is submitted. */
export interface ServerDraft {
  host: string;
  port: string;
  security: Security;
  username: string;
}

export interface ServersDraft {
  imap: ServerDraft;
  smtp: ServerDraft;
}

export type ServerKind = keyof ServersDraft;

/** Standard ports (RFC 8314 prefers implicit TLS; STARTTLS stays possible). */
export const DEFAULT_PORTS: Record<ServerKind, Record<Security, number>> = {
  imap: { tls: 993, starttls: 143 },
  smtp: { tls: 465, starttls: 587 },
};

export function emptyServers(email: string): ServersDraft {
  const blank = (kind: ServerKind): ServerDraft => ({
    host: "",
    port: String(DEFAULT_PORTS[kind].tls),
    security: "tls",
    username: email.trim(),
  });
  return { imap: blank("imap"), smtp: blank("smtp") };
}

export function fromSettings(settings: ServerSettings | null, kind: ServerKind, email: string): ServerDraft {
  if (!settings) return emptyServers(email)[kind];
  return {
    host: settings.host,
    port: String(settings.port),
    security: settings.security,
    username: settings.username || email.trim(),
  };
}

/** Proposed settings from discovery; a server it did not find stays empty for the user. */
export function fromDiscovery(result: DiscoveryResult, email: string): ServersDraft {
  return { imap: fromSettings(result.imap, "imap", email), smtp: fromSettings(result.smtp, "smtp", email) };
}

/** Changing the security also changes a port that still has the other mode's default. */
export function withSecurity(draft: ServerDraft, kind: ServerKind, security: Security): ServerDraft {
  const other: Security = security === "tls" ? "starttls" : "tls";
  const port = draft.port.trim() === String(DEFAULT_PORTS[kind][other]) ? String(DEFAULT_PORTS[kind][security]) : draft.port;
  return { ...draft, security, port };
}

/** Plausible address: something before and a domain after the @. The core validates again. */
export function isEmail(value: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@.]+$/.test(value.trim());
}

export function domainOf(email: string): string {
  return email.trim().split("@")[1]?.toLowerCase() ?? "";
}

export function parsePort(value: string): number | null {
  if (!/^\d{1,5}$/.test(value.trim())) return null;
  const port = Number(value.trim());
  return port >= 1 && port <= 65535 ? port : null;
}

export interface ServerErrors {
  host: boolean;
  port: boolean;
  username: boolean;
}

export function serverErrors(draft: ServerDraft): ServerErrors {
  return { host: draft.host.trim() === "", port: parsePort(draft.port) === null, username: draft.username.trim() === "" };
}

export function hasErrors(errors: ServerErrors): boolean {
  return errors.host || errors.port || errors.username;
}

export function toSettings(email: string, displayName: string, servers: ServersDraft): AccountSettings | null {
  const server = (draft: ServerDraft): ServerSettings | null => {
    const port = parsePort(draft.port);
    if (port === null || hasErrors(serverErrors(draft))) return null;
    return { host: draft.host.trim(), port, security: draft.security, username: draft.username.trim() };
  };
  const imap = server(servers.imap);
  const smtp = server(servers.smtp);
  if (!isEmail(email) || !imap || !smtp) return null;
  return { email: email.trim(), displayName: displayName.trim(), imap, smtp };
}

/** Security review N-4: why a server the online search found deserves a second look. */
export type ServerWarning = NonNullable<NonNullable<DiscoveryResult["warnings"]>["imap"]>;

function sameHost(a: string, b: string): boolean {
  const name = (host: string) => host.trim().replace(/\.$/, "").toLowerCase();
  return name(a) === name(b);
}

/**
 * Security review N-4: the core's warning for a server the online search found (outside the
 * address's domain, or an IP address), as long as the form still names that host. A host the
 * user typed in gets none, and the built-in table never has any.
 */
export function foundServerWarning(
  found: DiscoveryResult | null,
  kind: ServerKind | "jmap",
  host: string,
): ServerWarning | null {
  const warning = found?.warnings?.[kind];
  const server = found?.[kind];
  if (!warning || !server) return null;
  return sameHost(host, server.host) ? warning : null;
}

/** Stage 14 (decision 0034): Gmail's servers (also for Google Workspace domains), as the core's `is_gmail_host`. */
export function isGmailHost(host: string): boolean {
  const name = host.trim().replace(/\.$/, "").toLowerCase();
  return ["gmail.com", "googlemail.com"].some((domain) => name === domain || name.endsWith(`.${domain}`));
}

/** Stage 14 (decision 0041): the JMAP server of the form (HTTPS, port 443 unless given). */
export function emptyJmap(email: string): ServerDraft {
  return { host: "", port: "443", security: "tls", username: email.trim() };
}

/** The JMAP server discovery found, if any. */
export function jmapFromDiscovery(result: DiscoveryResult | undefined, email: string): ServerDraft | null {
  const found = result?.jmap;
  if (!found || found.host.trim() === "") return null;
  return { host: found.host, port: String(found.port), security: "tls", username: found.username || email.trim() };
}

/**
 * Settings of a JMAP account: one server, always TLS; the core uses it for sending too. Stage 14:
 * `credential` says whether the secret is an API token (left out for a password).
 */
export function toJmapSettings(
  email: string,
  displayName: string,
  draft: ServerDraft,
  credential: CredentialKind = "password",
): AccountSettings | null {
  const port = parsePort(draft.port);
  if (port === null || hasErrors(serverErrors(draft)) || !isEmail(email)) return null;
  const server: ServerSettings = { host: draft.host.trim(), port, security: "tls", username: draft.username.trim() };
  const settings: AccountSettings = { email: email.trim(), displayName: displayName.trim(), imap: server, smtp: server, protocol: "jmap" };
  return credential === "apiToken" ? { ...settings, credential } : settings;
}
