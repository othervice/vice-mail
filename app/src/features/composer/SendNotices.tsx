import { CircleCheck, CircleExclamationPoint, CircleQuestionMark, Clock, Send, X, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { composeIpc, ipc, toCoreError, type DraftSummary, type OutgoingMail } from "../../services/ipc";
import { openComposer, useComposerStore, type ComposerProblem, type TrackedSend } from "../../stores/composer";
import { useUiStore } from "../../stores/ui";
import { useDraftsAtStart } from "./queries";

/** A "Sent" notice goes after this long; questions and failures stay until the user acts. */
export const SENT_NOTICE_MS = 6000;

/** Re-renders every half second while `active` (the countdown of the undo delay). */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return undefined;
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, 500);
    return () => {
      window.clearInterval(timer);
    };
  }, [active]);
  return now;
}

/** "Undo" in the delay, "Edit" of a failed or unclear send: the draft is editable again. */
export async function reopenDraft(mail: OutgoingMail, context: "undo" | "edit"): Promise<boolean> {
  const store = useComposerStore.getState();
  if (!mail.draftId) return false;
  try {
    const draft = await composeIpc.cancelSend(mail.draftId);
    store.dismiss(mail.operationId);
    openComposer({ kind: "draft", draftId: draft.id, draft });
    return true;
  } catch (e) {
    store.report(context, toCoreError(e));
    return false;
  }
}

/** "It arrived" or (after confirming) "Send again" for a send whose outcome is unclear. */
export async function resolveUnclear(mail: OutgoingMail, decision: "treat_as_sent" | "send_again"): Promise<boolean> {
  const store = useComposerStore.getState();
  try {
    const next = await composeIpc.resolveUnclearSend(mail.operationId, decision);
    store.update([next]);
    store.setConfirmingSendAgain(mail.operationId, false);
    return true;
  } catch (e) {
    store.report("resolve", toCoreError(e));
    return false;
  }
}

/** "Retry" of a failed send: exactly what failed goes out again. Refused for unclear sends. */
export async function retrySend(mail: OutgoingMail): Promise<boolean> {
  try {
    await ipc.retryOperation(mail.operationId);
    return true;
  } catch (e) {
    useComposerStore.getState().report("retry", toCoreError(e));
    return false;
  }
}

interface NoticeButton {
  label: string;
  run: () => void;
  testId: string;
  tone?: "danger";
}

function SendNotice({ send }: { send: TrackedSend }) {
  const { t } = useTranslation();
  const dismiss = useComposerStore((state) => state.dismiss);
  const setConfirming = useComposerStore((state) => state.setConfirmingSendAgain);
  const showOutbox = useUiStore((state) => state.showOutbox);
  const { mail, confirmingSendAgain } = send;
  const [busy, setBusy] = useState(false);
  const now = useNow(mail.state === "scheduled");

  useEffect(() => {
    if (mail.state !== "sent") return undefined;
    const timer = window.setTimeout(() => {
      dismiss(mail.operationId);
    }, SENT_NOTICE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [mail.state, mail.operationId, dismiss]);

  const act = (run: () => Promise<unknown>) => () => {
    setBusy(true);
    void run().finally(() => {
      setBusy(false);
    });
  };
  const subject = mail.subject.trim() || t("composer.noSubject");
  let icon: LucideIcon = Send;
  let tone: "calm" | "success" | "danger" | "warning" = "calm";
  let text: string;
  let detail: string | null = null;
  let buttons: NoticeButton[] = [];
  switch (mail.state) {
    case "scheduled": {
      const seconds = Math.max(0, Math.ceil((mail.sendAt - now) / 1000));
      icon = Clock;
      text = seconds > 0 ? t("composer.send.scheduled", { count: seconds }) : t("composer.send.sending");
      if (seconds > 0)
        buttons = [{ label: t("composer.send.undo"), run: act(() => reopenDraft(mail, "undo")), testId: "send-undo" }];
      break;
    }
    case "queued":
      icon = Clock;
      text = t("composer.send.queued");
      if (mail.error) detail = t("composer.send.queuedOffline", { reason: t(`errors.codes.${mail.error.code}.title`) });
      break;
    case "sending":
      text = t("composer.send.sending");
      break;
    case "verifying":
      icon = CircleQuestionMark;
      text = t("composer.send.verifying");
      break;
    case "sent":
      icon = CircleCheck;
      tone = "success";
      text = t("composer.send.sent");
      if (mail.notice === "no_sent_copy") detail = t("composer.send.noSentCopy");
      break;
    case "failed":
      icon = CircleExclamationPoint;
      tone = "danger";
      text = t("composer.send.failed");
      if (mail.error) detail = `${t(`errors.codes.${mail.error.code}.title`)} ${t(`errors.codes.${mail.error.code}.hint`)}`;
      buttons = [
        { label: t("composer.outbox.retry"), run: act(() => retrySend(mail)), testId: "send-retry" },
        ...(mail.draftId ? [{ label: t("composer.outbox.edit"), run: act(() => reopenDraft(mail, "edit")), testId: "send-edit" }] : []),
        { label: t("composer.outbox.show"), run: showOutbox, testId: "send-show-outbox" },
      ];
      break;
    case "missed":
      // Stage 10: a scheduled send was due while the app was off; the outbox asks what to do.
      icon = Clock;
      tone = "warning";
      text = t("composer.send.missed");
      buttons = [{ label: t("composer.outbox.show"), run: showOutbox, testId: "send-show-outbox" }];
      break;
    case "unclear":
      icon = CircleQuestionMark;
      tone = "warning";
      text = t("composer.unclear.title");
      detail = confirmingSendAgain ? t("composer.unclear.confirmBody") : t("composer.unclear.body");
      buttons = confirmingSendAgain
        ? [
            {
              label: t("composer.unclear.confirmSendAgain"),
              run: act(() => resolveUnclear(mail, "send_again")),
              testId: "unclear-confirm-send-again",
              tone: "danger",
            },
            {
              label: t("composer.unclear.cancel"),
              run: () => {
                setConfirming(mail.operationId, false);
              },
              testId: "unclear-cancel",
            },
          ]
        : [
            { label: t("composer.unclear.arrived"), run: act(() => resolveUnclear(mail, "treat_as_sent")), testId: "unclear-arrived" },
            {
              label: t("composer.unclear.sendAgain"),
              run: () => {
                setConfirming(mail.operationId, true);
              },
              testId: "unclear-send-again",
            },
          ];
      break;
  }

  const needsUser = mail.state === "failed" || mail.state === "unclear" || mail.state === "missed";
  return (
    <div
      role={needsUser ? "alert" : undefined}
      className={cx(
        "pointer-events-auto flex w-full max-w-(--ui-notice-width) items-start gap-3 rounded-card border bg-surface-raised px-4 py-3 shadow-floating",
        tone === "danger" ? "border-danger" : tone === "warning" ? "border-warning" : "border-border",
      )}
      data-testid="send-notice"
      data-send-state={mail.state}
    >
      <Icon
        icon={icon}
        className={tone === "danger" ? "text-danger" : tone === "warning" ? "text-warning" : tone === "success" ? "text-success" : "text-accent"}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-13 font-medium text-primary">{text}</p>
        <p className="truncate text-12 text-secondary">{subject}</p>
        {detail && <p className="text-12 text-secondary">{detail}</p>}
        {buttons.length > 0 && (
          <div className="flex flex-wrap gap-1 pt-1">
            {buttons.map((button) => (
              <button
                key={button.testId}
                type="button"
                disabled={busy}
                onClick={button.run}
                className={cx(
                  "rounded-control px-2 py-1 text-13 font-medium hover:bg-hover disabled:opacity-50",
                  button.tone === "danger" ? "text-danger" : "text-accent",
                )}
                data-testid={button.testId}
              >
                {button.label}
              </button>
            ))}
          </div>
        )}
      </div>
      {mail.state !== "unclear" && (
        <button
          type="button"
          aria-label={t("actions.dismiss")}
          onClick={() => {
            dismiss(mail.operationId);
          }}
          className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          data-testid="send-dismiss"
        >
          <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </button>
      )}
    </div>
  );
}

function Icon({ icon: Glyph, className }: { icon: LucideIcon; className: string }) {
  return <Glyph aria-hidden className={cx("size-(--ui-icon-small) shrink-0", className)} strokeWidth={1.75} />;
}

function ProblemNotice({ problem, onDismiss }: { problem: ComposerProblem; onDismiss?: () => void }) {
  const { t } = useTranslation();
  const dismissProblem = useComposerStore((state) => state.dismissProblem);
  const dismiss = onDismiss ?? (() => {
    dismissProblem(problem.id);
  });
  return (
    <div
      role="alert"
      className="pointer-events-auto flex w-full max-w-(--ui-notice-width) items-start gap-3 rounded-card border border-danger bg-surface-raised px-4 py-3 shadow-floating"
      data-testid="composer-problem"
      data-error-code={problem.error.code}
    >
      <Icon icon={CircleExclamationPoint} className="text-danger" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-13 font-medium text-primary">{t(`composer.problems.${problem.context}`)}</p>
        <p className="text-12 text-secondary">{t(`errors.codes.${problem.error.code}.title`)}</p>
        <p className="text-12 text-secondary">{t(`errors.codes.${problem.error.code}.hint`)}</p>
      </div>
      <button
        type="button"
        aria-label={t("actions.dismiss")}
        onClick={dismiss}
        className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
      >
        <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
    </div>
  );
}

/**
 * After a crash or quitting with the composer open: the unsent local drafts of the last session,
 * offered once at the start (the newest opens in the composer). Drafts of this session are not.
 */
function DraftRestore() {
  const { t } = useTranslation();
  const drafts = useDraftsAtStart();
  const hidden = useComposerStore((state) => state.restoreDone);
  const finish = useComposerStore((state) => state.finishRestore);
  const [errorDismissed, setErrorDismissed] = useState(false);
  if (hidden) return null;
  if (drafts.error) {
    if (errorDismissed) return null;
    return (
      <ProblemNotice
        problem={{ id: 0, context: "restore", error: drafts.error }}
        onDismiss={() => {
          setErrorDismissed(true);
        }}
      />
    );
  }
  const offered: readonly DraftSummary[] = (drafts.data ?? []).filter((d) => d.state === "editing");
  const newest = offered[0];
  if (!newest) return null;
  return (
    <div
      className="pointer-events-auto flex w-full max-w-(--ui-notice-width) items-start gap-3 rounded-card border border-border bg-surface-raised px-4 py-3 shadow-floating"
      data-testid="draft-restore"
    >
      <Icon icon={Clock} className="text-accent" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-13 font-medium text-primary">{t("composer.restore.title", { count: offered.length })}</p>
        <p className="truncate text-12 text-secondary">{newest.subject.trim() || t("composer.noSubject")}</p>
        <div className="flex flex-wrap gap-1 pt-1">
          <button
            type="button"
            onClick={() => {
              openComposer({ kind: "draft", draftId: newest.id });
            }}
            className="rounded-control px-2 py-1 text-13 font-medium text-accent hover:bg-hover"
            data-testid="draft-restore-open"
          >
            {t("composer.restore.open")}
          </button>
          <button
            type="button"
            onClick={finish}
            className="rounded-control px-2 py-1 text-13 font-medium text-accent hover:bg-hover"
            data-testid="draft-restore-dismiss"
          >
            {t("composer.restore.dismiss")}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * The state of each send the user started, at the bottom right: the undo countdown, "Sending",
 * "Sent", failures with "Retry" and "Edit", and the question after an unclear outcome. A send is
 * never repeated without the user's explicit, confirmed choice.
 */
export function SendNotices() {
  const { t } = useTranslation();
  const sends = useComposerStore((state) => state.sends);
  const problems = useComposerStore((state) => state.problems);
  const calm = sends.filter((s) => s.mail.state !== "failed" && s.mail.state !== "unclear");
  const urgent = sends.filter((s) => s.mail.state === "failed" || s.mail.state === "unclear");
  return (
    <div
      role="group"
      aria-label={t("composer.send.notices")}
      className="pointer-events-none fixed end-6 bottom-6 z-30 flex w-(--ui-notice-width) max-w-full flex-col items-end gap-2"
      data-testid="send-notices"
    >
      <div role="status" className="flex w-full flex-col items-end gap-2">
        <DraftRestore />
        {calm.map((send) => (
          <SendNotice key={send.mail.operationId} send={send} />
        ))}
      </div>
      {urgent.map((send) => (
        <SendNotice key={send.mail.operationId} send={send} />
      ))}
      {problems.map((problem) => (
        <ProblemNotice key={problem.id} problem={problem} />
      ))}
    </div>
  );
}
