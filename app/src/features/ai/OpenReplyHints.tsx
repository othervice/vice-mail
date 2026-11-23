import { MessageCircleQuestionMark, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import type { MessageId, OpenReplyHint } from "../../services/ipc";
import { openComposer } from "../../stores/composer";
import { formatFullDate } from "../messages/rows";
import { useDismissOpenReplyHint, useOpenReplyHints } from "./queries";

/** "Maybe still open": a suggestion in its words, never a statement that something is unanswered. */
function useHintText() {
  const { t, i18n } = useTranslation();
  return (hint: OpenReplyHint) => {
    const who = hint.counterpart ? hint.counterpart.name || hint.counterpart.email : t("ai.openReplies.someone");
    return {
      title: t(`ai.openReplies.kind.${hint.kind}`, { who, count: hint.days }),
      detail: t("ai.openReplies.detail", {
        subject: hint.subject || t("list.noSubject"),
        date: formatFullDate(hint.date * 1000, i18n.language),
      }),
    };
  };
}

function HintActions({ hint }: { hint: OpenReplyHint }) {
  const { t } = useTranslation();
  const dismiss = useDismissOpenReplyHint();
  return (
    <>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          onClick={() => {
            openComposer({ kind: "reply", messageId: hint.messageId });
          }}
          data-testid="open-reply-answer"
        >
          {hint.kind === "awaiting_reply" ? t("ai.openReplies.followUp") : t("ai.openReplies.reply")}
        </Button>
        <Button
          variant="quiet"
          disabled={dismiss.isPending}
          onClick={() => {
            dismiss.mutate(hint.messageId);
          }}
          data-testid="open-reply-dismiss"
        >
          {t("ai.openReplies.dismiss")}
        </Button>
      </div>
      {dismiss.isError && <ErrorNotice context={t("ai.openReplies.dismissFailed")} error={dismiss.error} />}
    </>
  );
}

/** All current hints (newest first), for the AI panel while no thread is chosen. */
export function OpenReplyHintList() {
  const { t } = useTranslation();
  const hints = useOpenReplyHints();
  const text = useHintText();
  if (hints.isError) {
    return (
      <ErrorNotice
        context={t("ai.openReplies.loadFailed")}
        error={hints.error}
        onRetry={() => {
          void hints.refetch();
        }}
      />
    );
  }
  const list = hints.data ?? [];
  if (list.length === 0) return null;
  return (
    <section aria-label={t("ai.openReplies.listTitle")} className="flex flex-col gap-2" data-testid="open-reply-list">
      <h3 className="text-13 font-semibold text-primary">{t("ai.openReplies.listTitle")}</h3>
      <p className="text-12 text-secondary">{t("ai.openReplies.listHint")}</p>
      <ul className="flex flex-col gap-2">
        {list.map((hint) => {
          const { title, detail } = text(hint);
          return (
            <li key={hint.messageId} className="flex flex-col gap-2 rounded-card border border-border bg-canvas p-3" data-testid="open-reply-hint">
              <p className="text-13 text-primary">{title}</p>
              <p className="text-12 text-secondary">{detail}</p>
              <HintActions hint={hint} />
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/**
 * Above an opened message that may still wait for an answer. Works without AI (the rules run
 * in the core); dismissed hints never come back.
 */
export function OpenReplyHintBar({ messageId }: { messageId: MessageId }) {
  const { t } = useTranslation();
  const hints = useOpenReplyHints();
  const dismiss = useDismissOpenReplyHint();
  const text = useHintText();
  const hint = hints.data?.find((candidate) => candidate.messageId === messageId);
  if (!hint) return null;
  const { title } = text(hint);
  return (
    <div className="mx-6 mt-3 flex flex-col gap-2 rounded-card border border-border bg-priority-tint p-3" data-print-hide data-testid="open-reply-bar">
      <div className="flex items-start gap-3">
        <MessageCircleQuestionMark aria-hidden className="mt-0.5 size-(--ui-icon) shrink-0 text-secondary" strokeWidth={1.75} />
        <div role="note" aria-label={t("ai.openReplies.barLabel")} className="flex min-w-0 flex-1 flex-col gap-1">
          <p className="text-13 text-primary">{title}</p>
          <p className="text-12 text-secondary">{t("ai.openReplies.barHint")}</p>
        </div>
        <IconButton
          icon={X}
          label={t("ai.openReplies.dismiss")}
          disabled={dismiss.isPending}
          onClick={() => {
            dismiss.mutate(hint.messageId);
          }}
        />
      </div>
      {dismiss.isError && <ErrorNotice context={t("ai.openReplies.dismissFailed")} error={dismiss.error} />}
    </div>
  );
}
