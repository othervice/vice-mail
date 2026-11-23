import type { AccountId, ComposeRequest } from "../../services/ipc";
import type { ComposerSource } from "../../stores/composer";

/** The composer's source for a `mailto:` link (or an unsubscribe by mail), from one account. */
export function composerSourceFor(request: ComposeRequest, accountId: AccountId): ComposerSource {
  return {
    kind: "new",
    accountId,
    to: request.to,
    prefill: {
      cc: request.cc,
      bcc: request.bcc,
      subject: request.subject,
      body: request.body,
      inReplyTo: request.inReplyTo,
      notices: request.notices,
    },
  };
}
