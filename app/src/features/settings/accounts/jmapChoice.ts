import { useState } from "react";
import type { AccountSettings, DiscoveryResult, Protocol } from "../../../services/ipc";
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
}

/**
 * Stage 14 (decision 0041): the protocol choice of the account setup. IMAP is the default; JMAP
 * is offered in the server settings, prefilled when the online search found a JMAP server.
 */
export function useJmapChoice(email: string, displayName: string, discovered: DiscoveryResult | undefined): JmapChoice {
  const [protocol, setProtocol] = useState<Protocol>("imap");
  const [edited, setDraft] = useState<ServerDraft | null>(null);
  const found = jmapFromDiscovery(discovered, email);
  const draft = edited ?? found ?? emptyJmap(email);
  const settings = protocol === "jmap" ? toJmapSettings(email, displayName, draft) : null;
  return { protocol, setProtocol, draft, setDraft, settings, found };
}
