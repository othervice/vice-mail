import { useQueryClient } from "@tanstack/react-query";
import { ShieldAlert, ShieldX } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ipc, toCoreError, type Account, type CoreError, type MailAction, type MessageId, type SpamVerdict } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { refreshSpamMarks, useMarkSpamLocally, useSpamVerdict } from "./queries";

/**
 * Stage 22 (decision 0077): a quiet note on a message the learning spam filter moved to the junk
 * folder ("Not spam" moves it back and teaches the filter) or suspects on this device (an account
 * that may not write or has no junk folder: "Not spam" removes the mark, "Move to junk" where it
 * can). The score says how sure the filter was.
 */
export function SpamNote({ account, messageId }: { account: Account; messageId: MessageId }) {
  const verdict = useSpamVerdict(messageId);
  if (!verdict.data) return null;
  return <Note account={account} verdict={verdict.data} />;
}

function Note({ account, verdict }: { account: Account; verdict: SpamVerdict }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  const remember = useNoticeStore((state) => state.remember);
  const local = useMarkSpamLocally();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);
  const moved = verdict.mark === "moved";
  // Never "100 %": the filter is never certain.
  const percent = new Intl.NumberFormat(i18n.language, { style: "percent", maximumFractionDigits: 1 }).format(
    Math.min(verdict.score ?? 0, 0.999),
  );
  const Icon = moved ? ShieldX : ShieldAlert;

  /** "Not spam" (out of the junk folder) or "Move to junk" through the outbox, with undo. */
  const apply = (action: MailAction) => {
    const mailboxId = verdict.mailboxId;
    if (mailboxId === null) return;
    setBusy(true);
    setError(null);
    ipc
      .applyAction(action, [{ mailboxId, messageId: verdict.messageId }], crypto.randomUUID())
      .then(
        (result) => {
          if (result.batch !== null) remember([result.batch]);
          push({
            kind: "done",
            action: action.type === "spam" ? "spam" : "not_spam",
            count: 1,
            undo: result.batch === null ? null : { accountId: account.id, batches: [result.batch] },
          });
        },
        (e: unknown) => {
          const failure = toCoreError(e);
          if (failure.code === "read_only") push({ kind: "readOnly", accountId: account.id });
          else setError(failure);
        },
      )
      .finally(() => {
        setBusy(false);
        refreshSpamMarks(queryClient);
      });
  };

  return (
    <div
      role="note"
      data-testid="spam-note"
      data-mark={verdict.mark}
      data-print-hide
      className="flex flex-col gap-2 rounded-card border border-border-subtle bg-canvas px-3 py-2"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <Icon
          aria-hidden
          className={moved ? "size-(--ui-icon-small) shrink-0 text-secondary" : "size-(--ui-icon-small) shrink-0 text-warning"}
          strokeWidth={1.75}
        />
        <p className="min-w-0 flex-1 text-13 text-secondary">
          {moved ? t("spam.note.moved", { percent }) : t("spam.note.suspected", { percent })}
        </p>
        <div className="flex flex-wrap gap-2">
          {moved ? (
            <Button
              variant="quiet"
              disabled={busy || verdict.mailboxId === null}
              onClick={() => {
                apply({ type: "notSpam" });
              }}
              data-testid="spam-note-not-spam"
            >
              {t("spam.note.notSpam")}
            </Button>
          ) : (
            <>
              <Button
                variant="quiet"
                disabled={local.isPending}
                onClick={() => {
                  local.mutate(
                    { messageIds: [verdict.messageId], spam: false },
                    {
                      onSuccess: () => {
                        push({ kind: "comfort", notice: { type: "spamCleared", messageIds: [verdict.messageId] } });
                      },
                    },
                  );
                }}
                data-testid="spam-note-not-spam"
              >
                {t("spam.note.notSpam")}
              </Button>
              {verdict.canMove && (
                <Button
                  variant="quiet"
                  disabled={busy || verdict.mailboxId === null}
                  onClick={() => {
                    apply({ type: "spam" });
                  }}
                  data-testid="spam-note-move"
                >
                  {t("spam.note.move")}
                </Button>
              )}
            </>
          )}
        </div>
      </div>
      {!moved && !verdict.canMove && <p className="text-12 text-tertiary">{t("spam.note.localOnly")}</p>}
      {error && <ErrorNotice context={t("spam.note.failed")} error={error} />}
      {local.error && <ErrorNotice context={t("spam.note.failed")} error={local.error} />}
    </div>
  );
}
