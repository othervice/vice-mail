import { SendHorizontal } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ViewHeader } from "../../components/ViewHeader";
import type { Account, OutgoingMail, SendState } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { useAccounts } from "../accounts/queries";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { addressLabel } from "./addresses";
import { useOutgoing } from "./queries";
import { reopenDraft, resolveUnclear, retrySend } from "./SendNotices";

const STATE_TONE: Record<SendState, string> = {
  scheduled: "text-secondary",
  queued: "text-secondary",
  sending: "text-accent",
  verifying: "text-warning",
  unclear: "text-warning",
  failed: "text-danger",
  sent: "text-success",
};

function OutgoingRow({ mail, account, showAccount }: { mail: OutgoingMail; account: Account | undefined; showAccount: boolean }) {
  const { t, i18n } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const time = new Intl.DateTimeFormat(i18n.language, { dateStyle: "short", timeStyle: "short" }).format(mail.updatedAt);
  const names = mail.to.map(addressLabel).join(", ");
  const more = mail.recipientCount - mail.to.length;
  const act = (run: () => Promise<unknown>) => () => {
    setBusy(true);
    void run().finally(() => {
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
          {t(`composer.outbox.state.${mail.state}`)}
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
      {mail.state === "unclear" && (
        <p className="text-13 text-secondary">{confirming ? t("composer.unclear.confirmBody") : t("composer.unclear.body")}</p>
      )}
      {mail.state === "sent" && mail.notice === "no_sent_copy" && <p className="text-13 text-secondary">{t("composer.send.noSentCopy")}</p>}
      <div className="flex flex-wrap gap-2">
        {mail.state === "scheduled" && mail.draftId && (
          <Button variant="secondary" disabled={busy} onClick={act(() => reopenDraft(mail, "undo"))} data-testid="outgoing-undo">
            {t("composer.send.undo")}
          </Button>
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
      <ul aria-label={t("composer.outbox.title")} className="min-h-0 flex-1 overflow-y-auto" data-testid="outbox-list">
        {mails.map((mail) => (
          <OutgoingRow key={mail.operationId} mail={mail} account={byId.get(mail.accountId)} showAccount={list.length > 1} />
        ))}
      </ul>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="outbox-view">
      <ViewHeader title={t("composer.outbox.title")} subtitle={t("composer.outbox.subtitle")} dragRegion={dragRegion} />
      {body}
    </div>
  );
}
