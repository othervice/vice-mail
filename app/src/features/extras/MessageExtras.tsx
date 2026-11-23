import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { AccountId, MessageId } from "../../services/ipc";
import { InviteCard } from "./InviteCard";
import { useMessageExtras } from "./queries";
import { UnsubscribeBar } from "./UnsubscribeBar";

interface MessageExtrasProps {
  accountId: AccountId;
  messageId: MessageId;
}

/**
 * Stage 9 in the reading pane, below the head: "Unsubscribe" for list mail and the calendar
 * invitations of the message. Nothing when the message offers neither; the invitations appear
 * once the body is stored whole (asked again on `messageBodyAvailable`).
 */
export function MessageExtras({ accountId, messageId }: MessageExtrasProps) {
  const { t } = useTranslation();
  const extras = useMessageExtras(accountId, messageId);

  if (extras.isPending) return null;
  if (extras.isError) {
    return (
      <div data-print-hide>
        <ErrorNotice
          context={t("extras.loadFailed")}
          error={extras.error}
          onRetry={() => {
            void extras.refetch();
          }}
          data-testid="extras-error"
        />
      </div>
    );
  }
  const { unsubscribe, invites } = extras.data;
  if (invites.length === 0 && !unsubscribe.oneClickHost && !unsubscribe.mailto && !unsubscribe.web) return null;
  return (
    <div className="flex flex-col gap-3" data-testid="message-extras">
      {/* A new message: a fresh bar, without the answer of the last one. */}
      <UnsubscribeBar key={messageId} messageId={messageId} accountId={accountId} options={unsubscribe} />
      {invites.map((info) => (
        <InviteCard key={`${String(messageId)}/${String(info.partId)}`} messageId={messageId} info={info} />
      ))}
    </div>
  );
}
