import { CalendarClock, CircleCheck, CircleExclamationPoint, CircleQuestionMark, Clock, Send, X, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import {
  composeIpc,
  composerIpc,
  ipc,
  toCoreError,
  type CoreError,
  type DraftSummary,
  type MissedSendDecision,
  type OutgoingMail,
} from "../../services/ipc";
import { needsUser, openComposer, useComposerStore, type ComposerProblem, type TrackedSend } from "../../stores/composer";
import { useUiStore } from "../../stores/ui";
import { useDraftsAtStart, useScheduledSendsAtStart } from "./queries";
import { SendLaterPicker } from "./SendLaterPicker";

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

/**
 * Stage 10: the answer to a send planned for a time when the app was off (more than an hour ago).
 * "Keep as draft" takes it out of the outbox and opens the draft again.
 */
export async function resolveMissed(mail: OutgoingMail, decision: MissedSendDecision): Promise<boolean> {
  const store = useComposerStore.getState();
  try {
    const next = await composerIpc.resolveMissedSend(mail.operationId, decision);
    if (next) {
      store.update([next]);
    } else {
      store.dismiss(mail.operationId);
      if (mail.draftId) openComposer({ kind: "draft", draftId: mail.draftId });
    }
    return true;
  } catch (e) {
    store.report("resolve", toCoreError(e));
    return false;
  }
}

/** Stage 10: moves a send the user planned (`null` = send now, after the undo delay). Throws a `CoreError`. */
export async function rescheduleScheduled(mail: OutgoingMail, sendAt: number | null): Promise<OutgoingMail> {
  try {
    const next = await composerIpc.rescheduleSend(mail.operationId, sendAt === null ? null : { sendAt });
    useComposerStore.getState().update([next]);
    return next;
  } catch (e) {
    throw toCoreError(e);
  }
}

/** A planned time as the notices and the outbox show it ("Mon, 24 Nov, 08:00"). */
export function formatSendAt(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).format(ms);
}

interface NoticeButton {
  label: string;
  run: () => void;
  testId: string;
  tone?: "danger";
}

function SendNotice({ send }: { send: TrackedSend }) {
  const { t, i18n } = useTranslation();
  const dismiss = useComposerStore((state) => state.dismiss);
  const setConfirming = useComposerStore((state) => state.setConfirmingSendAgain);
  const showOutbox = useUiStore((state) => state.showOutbox);
  const { mail, confirmingSendAgain } = send;
  const [busy, setBusy] = useState(false);
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<CoreError | null>(null);
  // A time the user chose is no undo countdown.
  const now = useNow(mail.state === "scheduled" && !mail.scheduled);

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
  // Stage 16: a read receipt has no subject of the user's; it says what it is.
  const subject = mail.readReceipt ? t("composer.send.receipt") : mail.subject.trim() || t("composer.noSubject");
  let icon: LucideIcon = Send;
  let tone: "calm" | "success" | "danger" | "warning" = "calm";
  let text: string;
  let detail: string | null = null;
  let buttons: NoticeButton[] = [];
  switch (mail.state) {
    case "scheduled": {
      if (mail.scheduled) {
        // Stage 10: planned by the user ("send later"); it goes out only if the app runs then.
        icon = CalendarClock;
        text = t("sendLater.notice.scheduled", { time: formatSendAt(mail.sendAt, i18n.language) });
        detail = t("sendLater.hint");
        buttons = [{ label: t("composer.outbox.show"), run: showOutbox, testId: "send-show-outbox" }];
        break;
      }
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
      // Stage 16: "Send and archive" found no Archive folder; the original stayed where it was.
      if (mail.archive === "no_archive_mailbox") detail = [detail, t("composer.send.noArchive")].filter(Boolean).join(" ");
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
      // Stage 10: planned for a time when the app was off (over an hour ago). Nothing was sent.
      icon = CalendarClock;
      tone = "warning";
      text = t("sendLater.missed.title", { time: formatSendAt(mail.sendAt, i18n.language) });
      detail = t("sendLater.missed.body");
      buttons = picking
        ? []
        : [
            { label: t("sendLater.missed.sendNow"), run: act(() => resolveMissed(mail, { kind: "send_now" })), testId: "missed-send-now" },
            {
              label: t("sendLater.missed.reschedule"),
              run: () => {
                setPickError(null);
                setPicking(true);
              },
              testId: "missed-reschedule",
            },
            { label: t("sendLater.missed.keep"), run: act(() => resolveMissed(mail, { kind: "keep_as_draft" })), testId: "missed-keep" },
          ];
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

  const reschedule = (sendAt: number) => {
    setBusy(true);
    setPickError(null);
    composerIpc
      .resolveMissedSend(mail.operationId, { kind: "reschedule", sendAt })
      .then(
        (next) => {
          setPicking(false);
          if (next) useComposerStore.getState().update([next]);
        },
        (e: unknown) => {
          setPickError(toCoreError(e));
        },
      )
      .finally(() => {
        setBusy(false);
      });
  };
  return (
    <div
      role={needsUser(mail) ? "alert" : undefined}
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
        {picking && mail.state === "missed" && (
          <SendLaterPicker
            confirmLabel={t("sendLater.missed.rescheduleConfirm")}
            busy={busy}
            error={pickError}
            onChoose={reschedule}
            onCancel={() => {
              setPicking(false);
            }}
            data-testid="missed-picker"
          />
        )}
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
      {mail.state !== "unclear" && mail.state !== "missed" && (
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
        <p className="text-13 font-medium text-primary">
          {problem.context === "reschedule" ? t("sendLater.rescheduleFailed") : t(`composer.problems.${problem.context}`)}
        </p>
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
 * Stage 10: sends planned for a time when the app was off become `missed` while the core starts,
 * possibly before this window listened for `sendNeedsDecision`. So the list is asked once at the
 * start, and every missed send gets the question.
 */
function MissedAtStart() {
  const { t } = useTranslation();
  const scheduled = useScheduledSendsAtStart();
  const needsDecision = useComposerStore((state) => state.needsDecision);
  const [errorDismissed, setErrorDismissed] = useState(false);
  const data = scheduled.data;
  useEffect(() => {
    for (const mail of data ?? []) if (mail.state === "missed") needsDecision(mail);
  }, [data, needsDecision]);
  if (!scheduled.error || errorDismissed) return null;
  return (
    <div
      role="alert"
      className="pointer-events-auto flex w-full max-w-(--ui-notice-width) items-start gap-3 rounded-card border border-danger bg-surface-raised px-4 py-3 shadow-floating"
      data-testid="missed-list-error"
      data-error-code={scheduled.error.code}
    >
      <Icon icon={CircleExclamationPoint} className="text-danger" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-13 font-medium text-primary">{t("sendLater.listFailed")}</p>
        <p className="text-12 text-secondary">{t(`errors.codes.${scheduled.error.code}.title`)}</p>
        <p className="text-12 text-secondary">{t(`errors.codes.${scheduled.error.code}.hint`)}</p>
        <div className="flex flex-wrap gap-1 pt-1">
          <button
            type="button"
            onClick={() => void scheduled.refetch()}
            className="rounded-control px-2 py-1 text-13 font-medium text-accent hover:bg-hover"
          >
            {t("errors.retry")}
          </button>
        </div>
      </div>
      <button
        type="button"
        aria-label={t("actions.dismiss")}
        onClick={() => {
          setErrorDismissed(true);
        }}
        className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
      >
        <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </button>
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
  const calm = sends.filter((s) => !needsUser(s.mail));
  const urgent = sends.filter((s) => needsUser(s.mail));
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
      <MissedAtStart />
    </div>
  );
}
