import type { OAuthProvider, ProviderHint } from "../../../services/ipc";

/** Microsoft's IMAP servers for Exchange Online and Outlook.com (the core checks the same list). */
const MICROSOFT_IMAP_HOSTS = ["outlook.office365.com", "outlook.office.com"];
/** Gmail's IMAP server. */
const GOOGLE_IMAP_HOSTS = ["imap.gmail.com"];

function hostName(host: string): string {
  return host.trim().replace(/\.$/, "").toLowerCase();
}

/**
 * Stage 14 (decision 0042): the provider whose sign-in fits the address, from the provider table's
 * hint or the IMAP server (Microsoft 365 and Google Workspace domains are found by their servers).
 * The core decides again and only hands a token to the provider's own servers.
 */
export function oauthProviderFor(hint: ProviderHint | null, imapHost: string): OAuthProvider | null {
  const host = hostName(imapHost);
  if (MICROSOFT_IMAP_HOSTS.includes(host)) return "microsoft";
  if (GOOGLE_IMAP_HOSTS.includes(host)) return "google";
  if (hint === "outlook") return "microsoft";
  return null;
}
