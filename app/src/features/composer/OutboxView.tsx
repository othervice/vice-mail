import { useQueryClient } from "@tanstack/react-query";
import { CalendarClock, SendHorizontal } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ViewHeader } from "../../components/ViewHeader";
import { toCoreError, type Account, type CoreError, type OutgoingMail, type SendState } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { useAccounts } from "../accounts/queries";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { useComposerStore } from "../../stores/composer";
import { addressLabel } from "./addresses";
import { useOutgoing } from "./queries";
import { formatSendAt, reopenDraft, rescheduleScheduled, resolveMissed, resolveUnclear, retrySend } from "./SendNotices";
import { SendLaterPicker } from "./SendLaterPicker";

/** Stage 10: a send whose time the user chose, still waiting, or one the app missed. */
const isPlanned = (mail: OutgoingMail) => (mail.state === "scheduled" && mail.scheduled) || mail.state === "missed";

const STATE_TONE: Record<SendState, string> = {
  scheduled: "text-secondary",
  queued: "text-secondary",
  sending: "text-accent",
  verifying: "text-warning",
  unclear: "text-warning",
  failed: "text-danger",
  missed: "text-warning",
  sent: "text-success",
};

function OutgoingRow({ mail, account, showAccount }: { mail: OutgoingMail; account: Account | undefined; showAccount: boolean }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  // Stage 10: the time picker of "Change time" (planned) or "Schedule again" (missed).
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<CoreError | null>(null);
  const time = new Intl.DateTimeFormat(i18n.language, { dateStyle: "short", timeStyle: "short" }).format(mail.updatedAt);
  const names = mail.to.map(addressLabel).join(", ");
  const more = mail.recipientCount - mail.to.length;
  const planned = mail.state === "scheduled" && mail.scheduled;
  // The core also sends `outgoingChanged`; the list follows at once.
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["outgoing"] });
  const act = (run: () => Promise<unknown>) => () => {
    setBusy(true);
    void run()
      .then(refresh)
      .finally(() => {
        setBusy(false);
      });
  };
  const choose = (sendAt: number) => {
    setBusy(true);
    setPickError(null);
    const run =
      mail.state === "missed"
        ? resolveMissed(mail, { kind: "reschedule", sendAt }).then((ok) => {
            if (!ok) throw new Error("not resolved");
          })
        : rescheduleScheduled(mail, sendAt);
    run
      .then(
        () => {
          setPicking(false);
          return refresh();
        },
        (e: unknown) => {
          // `resolveMissed` reports its own failure; a move shows it in the picker.
          if (mail.state !== "missed") setPickError(e as CoreError);
        },
      )
      .finally(() => {
        setBusy(false);
      });
  };
  return (
    <li className="flex flex-col gap-2 border-b border-border px-6 py-3" data-testid="outgoing-row" data-send-state={mail.state}>
      <div className="flex min-w-0 items-baseline gap-3">
        {showAccount && account && (
          <span aria-hidden className={cx("size-(--ui-dot) shrink-0 self-center rounded-full", accountFill(account.color))} />
        )}
        <p className="min-w-0 flex-1 truncate text-14 font-medium text-primary">{mail.subject.trim() || t("composer.noSubject")}</p>
        <p className={cx("shrink-0 text-12 font-medium", STATE_TONE[mail.state])} data-testid="outgoing-state">
          {planned ? t("sendLater.outbox.state") : t(`composer.outbox.state.${mail.state}`)}
        </p>
        <p className="shrink-0 text-12 text-tertiary tabular-nums">{time}</p>
      </div>
      <p className="truncate text-13 text-secondary">
        {more > 0 ? t("composer.outbox.toMore", { names, count: more }) : t("composer.outbox.to", { names })}
        {showAccount && account && t("composer.outbox.fromAccount", { email: account.email })}
      </p>
      {mail.error && (mail.state === "failed" || mail.state === "queued") && (
        <p className="text-13 text-secondary" data-error-code={mail.error.code}>
          {mail.state === "queued"
            ? t("composer.send.queuedOffline", { reason: t(`errors.codes.${mail.error.code}.title`) })
            : `${t(`errors.codes.${mail.error.code}.title`)} ${t(`errors.codes.${mail.error.code}.hint`)}`}
        </p>
      )}
      {planned && (
        <p className="flex items-center gap-2 text-13 font-medium text-primary" data-testid="outgoing-planned">
          <CalendarClock aria-hidden className="size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
          {t("sendLater.outbox.plannedFor", { time: formatSendAt(mail.sendAt, i18n.language) })}
        </p>
      )}
      {mail.state === "missed" && (
        <div className="flex flex-col gap-1 text-13" data-testid="outgoing-missed">
          <p className="font-medium text-primary">{t("sendLater.missed.title", { time: formatSendAt(mail.sendAt, i18n.language) })}</p>
          <p className="text-secondary">{t("sendLater.missed.body")}</p>
        </div>
      )}
      {picking && isPlanned(mail) && (
        <SendLaterPicker
          initial={planned ? mail.sendAt : null}
          confirmLabel={planned ? t("sendLater.confirm") : t("sendLater.missed.rescheduleConfirm")}
          busy={busy}
          error={pickError}
          onChoose={choose}
          onCancel={() => {
            setPicking(false);
          }}
          data-testid="outgoing-picker"
        />
      )}
      {mail.state === "unclear" && (
        <p className="text-13 text-secondary">{confirming ? t("composer.unclear.confirmBody") : t("composer.unclear.body")}</p>
      )}
      {mail.state === "sent" && mail.notice === "no_sent_copy" && <p className="text-13 text-secondary">{t("composer.send.noSentCopy")}</p>}
      <div className="flex flex-wrap gap-2">
        {mail.state === "scheduled" && !mail.scheduled && mail.draftId && (
          <Button variant="secondary" disabled={busy} onClick={act(() => reopenDraft(mail, "undo"))} data-testid="outgoing-undo">
            {t("composer.send.undo")}
          </Button>
        )}
        {planned && !picking && (
          <>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => {
                setPickError(null);
                setPicking(true);
              }}
              data-testid="outgoing-change"
            >
              {t("sendLater.outbox.change")}
            </Button>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={act(() =>
                rescheduleScheduled(mail, null).catch((e: unknown) => {
                  useComposerStore.getState().report("reschedule", toCoreError(e));
                }),
              )}
              data-testid="outgoing-send-now"
            >
              {t("sendLater.outbox.sendNow")}
            </Button>
            {mail.draftId && (
              <Button variant="secondary" disabled={busy} onClick={act(() => reopenDraft(mail, "edit"))} data-testid="outgoing-cancel">
                {t("sendLater.outbox.cancel")}
              </Button>
            )}
          </>
        )}
        {mail.state === "missed" && !picking && (
          <>
            <Button variant="secondary" disabled={busy} onClick={act(() => resolveMissed(mail, { kind: "send_now" }))} data-testid="outgoing-missed-send-now">
              {t("sendLater.missed.sendNow")}
            </Button>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => {
                setPickError(null);
                setPicking(true);
              }}
              data-testid="outgoing-missed-reschedule"
            >
              {t("sendLater.missed.reschedule")}
            </Button>
            <Button variant="secondary" disabled={busy} onClick={act(() => resolveMissed(mail, { kind: "keep_as_draft" }))} data-testid="outgoing-missed-keep">
              {t("sendLater.missed.keep")}
            </Button>
          </>
        )}
        {mail.state === "failed" && (
          <Button variant="secondary" disabled={busy} onClick={act(() => retrySend(mail))} data-testid="outgoing-retry">
            {t("composer.outbox.retry")}
          </Button>
        )}
        {(mail.state === "failed" || mail.state === "unclear") && mail.draftId && !confirming && (
          <Button variant="secondary" disabled={busy} onClick={act(() => reopenDraft(mail, "edit"))} data-testid="outgoing-edit">
            {t("composer.outbox.edit")}
          </Button>
        )}
        {mail.state === "unclear" && !confirming && (
          <>
            <Button variant="secondary" disabled={busy} onClick={act(() => resolveUnclear(mail, "treat_as_sent"))} data-testid="outgoing-arrived">
              {t("composer.unclear.arrived")}
            </Button>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => {
                setConfirming(true);
              }}
              data-testid="outgoing-send-again"
            >
              {t("composer.unclear.sendAgain")}
            </Button>
          </>
        )}
        {mail.state === "unclear" && confirming && (
          <>
            <Button variant="danger" disabled={busy} onClick={act(() => resolveUnclear(mail, "send_again"))} data-testid="outgoing-confirm-send-again">
              {t("composer.unclear.confirmSendAgain")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setConfirming(false);
              }}
              data-testid="outgoing-cancel-send-again"
            >
              {t("composer.unclear.cancel")}
            </Button>
          </>
        )}
      </div>
    </li>
  );
}

/**
 * "Ausgang": every send of every account, newest first, with its state. Failed ones keep their
 * error with "Retry" and "Edit"; an unclear one waits for the user's answer; offline sends wait
 * as `queued` and go out once the account is connected again. Sent ones go after a day.
 */
export function OutboxView() {
  const { t } = useTranslation();
  const dragRegion = useWindowDragRegion();
  const accounts = useAccounts();
  const list = accounts.data ?? [];
  const results = useOutgoing(list);
  const byId = new Map(list.map((account) => [account.id, account]));
  const mails = results.flatMap((r) => r.data ?? []).sort((a, b) => b.createdAt - a.createdAt || b.operationId - a.operationId);
  const failed = results.find((r) => r.isError);
  const loading = accounts.isPending || results.some((r) => r.isPending);

  let body;
  if (accounts.isError) {
    body = (
      <div className="px-6 py-4">
        <ErrorNotice context={t("settings.accounts.loadFailed")} error={accounts.error} onRetry={() => void accounts.refetch()} />
      </div>
    );
  } else if (failed?.error) {
    body = (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("composer.outbox.loadFailed")}
          error={failed.error}
          onRetry={() => void failed.refetch()}
          data-testid="outbox-error"
        />
      </div>
    );
  } else if (loading) {
    body = null;
  } else if (mails.length === 0) {
    body = <EmptyState icon={SendHorizontal} title={t("composer.outbox.emptyTitle")} body={t("composer.outbox.emptyBody")} />;
  } else {
    body = (
      <>
        {mails.some(isPlanned) && (
          <p className="flex shrink-0 items-start gap-2 border-b border-border bg-canvas px-6 py-2 text-13 text-secondary" data-testid="outbox-send-later-rule">
            <CalendarClock aria-hidden className="mt-0.5 size-(--ui-icon-small) shrink-0 text-accent" strokeWidth={1.75} />
            {t("sendLater.outbox.rule")}
          </p>
        )}
        <ul aria-label={t("composer.outbox.title")} className="min-h-0 flex-1 overflow-y-auto" data-testid="outbox-list">
          {mails.map((mail) => (
            <OutgoingRow key={mail.operationId} mail={mail} account={byId.get(mail.accountId)} showAccount={list.length > 1} />
          ))}
        </ul>
      </>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="outbox-view">
      <ViewHeader title={t("composer.outbox.title")} subtitle={t("composer.outbox.subtitle")} dragRegion={dragRegion} />
      {body}
    </div>
  );
}
