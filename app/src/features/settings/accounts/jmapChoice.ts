import { useState } from "react";
import type { AccountSettings, CredentialKind, DiscoveryResult, Protocol } from "../../../services/ipc";
import { emptyJmap, jmapFromDiscovery, toJmapSettings, type ServerDraft } from "./serverDraft";

export interface JmapChoice {
  protocol: Protocol;
  setProtocol: (protocol: Protocol) => void;
  /** The JMAP server as the form shows it: edited, else found online, else empty. */
  draft: ServerDraft;
  setDraft: (draft: ServerDraft) => void;
  /** The account settings when JMAP is chosen and complete. */
  settings: AccountSettings | null;
  /** The JMAP server the online search found, if any. */
  found: ServerDraft | null;
  /** Stage 14: a password or an API token (only with JMAP). */
  credential: CredentialKind;
  setCredential: (credential: CredentialKind) => void;
  /** The API token as typed; the password field at the top is not used then. */
  token: string;
  setToken: (token: string) => void;
  /** JMAP with an API token is chosen. */
  usesToken: boolean;
}

/**
 * Stage 14 (decision 0041): the protocol choice of the account setup. IMAP is the default; JMAP
 * is offered in the server settings, prefilled when the online search found a JMAP server.
 */
export function useJmapChoice(email: string, displayName: string, discovered: DiscoveryResult | undefined): JmapChoice {
  const [protocol, setProtocol] = useState<Protocol>("imap");
  const [edited, setDraft] = useState<ServerDraft | null>(null);
  const [credential, setCredential] = useState<CredentialKind>("password");
  const [token, setToken] = useState("");
  const found = jmapFromDiscovery(discovered, email);
  const draft = edited ?? found ?? emptyJmap(email);
  const settings = protocol === "jmap" ? toJmapSettings(email, displayName, draft, credential) : null;
  const usesToken = protocol === "jmap" && credential === "apiToken";
  return { protocol, setProtocol, draft, setDraft, settings, found, credential, setCredential, token, setToken, usesToken };
}
