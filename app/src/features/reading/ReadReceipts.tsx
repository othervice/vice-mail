import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MailCheck } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import {
  parityIpc,
  type Account,
  type MessageId,
  type QuoteLanguage,
  type ReadReceiptInfo,
  type ReceivedReadReceipt,
} from "../../services/ipc";
import { useUiStore } from "../../stores/ui";

/** Stage 16 (RFC 8098): the read receipts of one shown message. */
export const readReceiptQueryKey = (messageId: MessageId) => ["readReceipt", messageId] as const;

/** The receipt's own text goes out in the app's language (English or German). */
function receiptLanguage(language: string): QuoteLanguage {
  return language.toLowerCase().startsWith("de") ? "de" : "en";
}

/** Asked once the body is shown: the core reads the request from the stored message. */
export function useReadReceipt(messageId: MessageId, bodyShown: boolean) {
  return useQuery({
    queryKey: readReceiptQueryKey(messageId),
    queryFn: () => parityIpc.getReadReceipt(messageId),
    enabled: bodyShown,
    // Receipts that came back arrive with their bodies: ask anew each time the message is shown.
    staleTime: 0,
  });
}

interface ReadReceiptsProps {
  account: Account;
  messageId: MessageId;
  /** The body is on screen (`getReadReceipt` reads the stored message). */
  bodyShown: boolean;
}

/**
 * What the reading pane says about read receipts: the sender's request with "Send receipt" and
 * "Don't send" (never answered by itself), the receipts that came back for a message the user
 * sent, and that a message is itself a receipt. Nothing while there is nothing to say.
 */
export function ReadReceipts({ account, messageId, bodyShown }: ReadReceiptsProps) {
  const { t } = useTranslation();
  const query = useReadReceipt(messageId, bodyShown);
  if (query.isError) {
    return (
      <ErrorNotice
        context={t("receipts.errors.load")}
        error={query.error}
        onRetry={() => void query.refetch()}
        data-testid="read-receipt-error"
      />
    );
  }
  const info = query.data;
  if (!info) return null;
  return (
    <>
      <ReceiptRequest account={account} messageId={messageId} info={info} />
      <ReceivedReceipts info={info} />
      {info.receiptFor !== null && (
        <p className="flex items-center gap-2 text-13 text-secondary" data-testid="read-receipt-for">
          <MailCheck aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
          {t("receipts.receiptFor")}
        </p>
      )}
    </>
  );
}

type Answered = "queued" | "sent" | "declined";

function ReceiptRequest({ account, messageId, info }: { account: Account; messageId: MessageId; info: ReadReceiptInfo }) {
  const { t, i18n } = useTranslation();
  const titleId = useId();
  const queryClient = useQueryClient();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  // What the user answered here; earlier answers (and "answered elsewhere", "off") say nothing.
  const [answered, setAnswered] = useState<Answered | null>(null);
  const send = useMutation({
    // A fresh request id per press; pressing twice returns the receipt already on its way.
    mutationFn: () => parityIpc.sendReadReceipt(messageId, receiptLanguage(i18n.language), crypto.randomUUID()),
    onSuccess: (mail) => {
      setAnswered(mail.state === "sent" ? "sent" : "queued");
      queryClient.setQueryData<ReadReceiptInfo>(readReceiptQueryKey(messageId), (current) =>
        current?.request ? { ...current, request: { ...current.request, answer: "queued" } } : current,
      );
    },
  });
  const decline = useMutation({
    mutationFn: () => parityIpc.declineReadReceipt(messageId, crypto.randomUUID()),
    onSuccess: (next) => {
      const answer = next.request?.answer;
      setAnswered(answer === "queued" || answer === "sent" ? answer : "declined");
      queryClient.setQueryData(readReceiptQueryKey(messageId), next);
    },
  });

  if (answered) {
    return (
      <p role="status" className="flex items-center gap-2 text-13 text-secondary" data-testid="read-receipt-answered" data-answer={answered}>
        <MailCheck aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
        {t(`receipts.request.${answered}`)}
      </p>
    );
  }
  const request = info.request;
  if (request?.answer !== "ask") return null;
  const busy = send.isPending || decline.isPending;
  const error = send.error ?? decline.error;
  return (
    <div
      role="group"
      aria-labelledby={titleId}
      className="flex flex-col gap-3 rounded-card border border-border bg-canvas p-3"
      data-testid="read-receipt-request"
      data-print-hide
    >
      <div className="flex items-start gap-3">
        <MailCheck aria-hidden className="size-(--ui-icon) shrink-0 text-secondary" strokeWidth={1.75} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <p id={titleId} className="font-semibold text-primary">
            {t("receipts.request.title")}
          </p>
          <p className="text-13 text-secondary">{t("receipts.request.body", { address: request.to })}</p>
          {request.returnPathDiffers && (
            <p className="text-13 text-warning" data-testid="read-receipt-return-path">
              {t("receipts.request.returnPathDiffers", { address: request.to })}
            </p>
          )}
        </div>
      </div>
      <div className="flex flex-wrap gap-2 ps-8">
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => {
            send.mutate();
          }}
          data-testid="read-receipt-send"
        >
          {t("receipts.request.send")}
        </Button>
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => {
            decline.mutate();
          }}
          data-testid="read-receipt-decline"
        >
          {t("receipts.request.decline")}
        </Button>
      </div>
      {error && (
        <ErrorNotice
          context={t(send.error ? "receipts.errors.send" : "receipts.errors.decline")}
          error={error}
          actions={
            error.code === "read_only" ? (
              <Button
                variant="secondary"
                onClick={() => {
                  showAccountsPage({ kind: "account", accountId: account.id });
                }}
                data-testid="read-receipt-allow-changes"
              >
                {t("actions.readOnly.open")}
              </Button>
            ) : undefined
          }
          data-testid="read-receipt-send-error"
        />
      )}
    </div>
  );
}

/** Receipts that came back for a message the user sent ("Displayed by bob@… on …"). */
function ReceivedReceipts({ info }: { info: ReadReceiptInfo }) {
  const { t, i18n } = useTranslation();
  if (info.received.length === 0 && !info.requested) return null;
  const date = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" });
  const line = (receipt: ReceivedReadReceipt) =>
    t(`receipts.received.${receipt.disposition}`, {
      recipient: receipt.recipient ?? t("receipts.received.someone"),
      date: date.format(receipt.receivedAt),
    });
  return (
    <section aria-label={t("receipts.received.label")} className="flex flex-col gap-1 text-13 text-secondary" data-testid="read-receipts-received">
      {info.received.length === 0 ? (
        <p className="flex items-center gap-2" data-testid="read-receipt-waiting">
          <MailCheck aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
          {t("receipts.received.waiting")}
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {info.received.map((receipt) => (
            <li key={`${receipt.recipient ?? ""}-${String(receipt.receivedAt)}`} className="flex items-center gap-2" data-testid="read-receipt-received">
              <MailCheck aria-hidden className="size-(--ui-icon-small) shrink-0 text-success" strokeWidth={1.75} />
              {line(receipt)}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
