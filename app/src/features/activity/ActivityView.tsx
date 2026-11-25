import { useQueryClient } from "@tanstack/react-query";
import { CircleCheck, Clock, History, TriangleExclamationPoint, type LucideIcon } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ViewHeader } from "../../components/ViewHeader";
import type { Account, Mailbox, MailboxId, OperationInfo, OperationState } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { useAccounts } from "../accounts/queries";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { outboxCountsQueryKey, operationsQueryKey, useDiscardOperations, useOperations, useRetryOperations } from "../actions/queries";
import { useMailboxes } from "../mailboxes/queries";
import { useUiStore } from "../../stores/ui";

/** States in the order a batch line names them. */
const STATE_ORDER: readonly OperationState[] = ["failed", "conflict", "queued", "in_flight", "confirmed"];

interface Batch {
  id: number;
  operations: OperationInfo[];
}

/** Operations of one batch (one user action) together, newest batch first. */
export function groupByBatch(operations: readonly OperationInfo[]): Batch[] {
  const batches = new Map<number, Batch>();
  for (const operation of operations) {
    const batch = batches.get(operation.batch) ?? { id: operation.batch, operations: [] };
    batch.operations.push(operation);
    batches.set(operation.batch, batch);
  }
  return [...batches.values()];
}

function stateIcon(states: ReadonlySet<OperationState>): { icon: LucideIcon; className: string } {
  if (states.has("failed")) return { icon: TriangleExclamationPoint, className: "text-danger" };
  if (states.has("conflict")) return { icon: TriangleExclamationPoint, className: "text-warning" };
  if (states.has("queued") || states.has("in_flight")) return { icon: Clock, className: "text-tertiary" };
  return { icon: CircleCheck, className: "text-success" };
}

function formatWhen(ms: number, language: string): string {
  const today = new Date();
  const date = new Date(ms);
  const sameDay = date.toDateString() === today.toDateString();
  return new Intl.DateTimeFormat(language, sameDay ? { timeStyle: "short" } : { dateStyle: "medium", timeStyle: "short" }).format(ms);
}

function useFolderName(mailboxes: readonly Mailbox[] | undefined) {
  const { t } = useTranslation();
  return (id: MailboxId | null): string | null => {
    if (id === null) return null;
    const mailbox = mailboxes?.find((m) => m.id === id);
    if (!mailbox) return t("activity.unknownFolder");
    return mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`);
  };
}

function distinct<T>(values: readonly (T | null)[]): T[] {
  return [...new Set(values.filter((value): value is T => value !== null))];
}

function BatchRow({ account, batch, mailboxes }: { account: Account; batch: Batch; mailboxes: readonly Mailbox[] | undefined }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const retry = useRetryOperations();
  const discard = useDiscardOperations();
  const folderName = useFolderName(mailboxes);
  const { operations } = batch;
  const first = operations[0];
  if (!first) return null;

  const states = new Set(operations.map((operation) => operation.state));
  const { icon: Icon, className } = stateIcon(states);
  // Stage 21: a folder marked as read is one operation for many messages.
  const messages =
    new Set(operations.flatMap((operation) => (operation.messageId === null ? [] : [operation.messageId]))).size +
    operations.reduce((sum, operation) => sum + (operation.messageCount ?? 0), 0);
  const from = distinct(operations.map((operation) => folderName(operation.mailboxId)));
  const to = distinct(operations.map((operation) => folderName(operation.targetMailboxId)));
  const errors = distinct(operations.map((operation) => (operation.state === "failed" ? (operation.error?.code ?? null) : null)));
  const conflicts = distinct(operations.map((operation) => operation.conflict));
  const notices = distinct(operations.map((operation) => operation.notice));
  const retryable = operations.filter((operation) => operation.state === "failed" || operation.state === "conflict");
  const finished = operations.filter((operation) => ["failed", "conflict", "confirmed"].includes(operation.state));
  const updated = Math.max(...operations.map((operation) => operation.updatedAt));

  const refresh = () => {
    void Promise.all([
      queryClient.invalidateQueries({ queryKey: operationsQueryKey(account.id) }),
      queryClient.invalidateQueries({ queryKey: outboxCountsQueryKey(account.id) }),
      queryClient.invalidateQueries({ queryKey: ["messages", account.id] }),
    ]);
  };

  const stateCounts = STATE_ORDER.flatMap((state) => {
    const count = operations.filter((operation) => operation.state === state).length;
    return count > 0 ? [t("activity.stateCount", { count, state: t(`activity.states.${state}`) })] : [];
  });

  return (
    <li className="flex gap-3 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4" data-testid={`activity-batch-${String(batch.id)}`}>
      <Icon aria-hidden className={cx("size-(--ui-icon) shrink-0", className)} strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
          <p className="flex flex-wrap items-baseline gap-x-2 text-14">
            <span className="font-semibold text-primary">{t(`activity.actions.${first.action}`)}</span>
            {messages > 0 && <span className="text-13 text-secondary">{t("activity.count", { count: messages })}</span>}
          </p>
          <time dateTime={new Date(updated).toISOString()} className="text-12 text-secondary tabular-nums">
            {formatWhen(updated, i18n.language)}
          </time>
        </div>
        {(from.length > 0 || to.length > 0) && (
          <p className="text-13 text-secondary">
            {to.length > 0 ? t("activity.folders", { from: from.join(", "), to: to.join(", ") }) : from.join(", ")}
          </p>
        )}
        <ul className="flex flex-wrap gap-2 text-12 font-medium text-secondary" data-testid="activity-states">
          {stateCounts.map((text) => (
            <li key={text} className="rounded-full bg-badge px-2">
              {text}
            </li>
          ))}
        </ul>
        {errors.map((code) => (
          <p key={code} className="text-13 text-danger" data-testid="activity-error">
            {t(`errors.codes.${code}.title`)} {t(`errors.codes.${code}.hint`)}
          </p>
        ))}
        {conflicts.map((reason) => (
          <p key={reason} className="text-13 text-secondary" data-testid="activity-conflict">
            {t(`activity.conflict.${reason}`)}
          </p>
        ))}
        {notices.map((notice) => (
          <p key={notice} className="text-13 text-secondary">
            {t(`activity.notice.${notice}`)}
          </p>
        ))}
        {(retryable.length > 0 || finished.length > 0) && (
          <div className="flex flex-wrap gap-2">
            {retryable.length > 0 && (
              <Button
                variant="secondary"
                disabled={retry.isPending}
                onClick={() => {
                  retry.mutate(
                    retryable.map((operation) => operation.id),
                    { onSettled: refresh },
                  );
                }}
                data-testid="activity-retry"
              >
                {t("activity.retry")}
              </Button>
            )}
            {finished.length > 0 && (
              <Button
                variant="quiet"
                disabled={discard.isPending}
                onClick={() => {
                  discard.mutate(
                    finished.map((operation) => operation.id),
                    { onSettled: refresh },
                  );
                }}
                data-testid="activity-discard"
              >
                {t("activity.discard")}
              </Button>
            )}
          </div>
        )}
        {retry.isError && <ErrorNotice context={t("activity.retryFailed")} error={retry.error} />}
        {discard.isError && <ErrorNotice context={t("activity.discardFailed")} error={discard.error} />}
      </div>
    </li>
  );
}

function AccountActivity({ account }: { account: Account }) {
  const { t } = useTranslation();
  const headingId = useId();
  const operations = useOperations(account.id);
  const mailboxes = useMailboxes(account.id);
  const batches = groupByBatch(operations.data?.pages.flat() ?? []);
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3" data-testid={`activity-${account.id}`}>
      <h2 id={headingId} className="flex min-w-0 items-center gap-2 text-14 font-semibold text-primary">
        <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
        <span className="truncate">
          {account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email}
        </span>
      </h2>
      {operations.isError && (
        <ErrorNotice
          context={t("activity.loadFailed")}
          error={operations.error}
          onRetry={() => {
            void operations.refetch();
          }}
        />
      )}
      {operations.isSuccess && batches.length === 0 && <p className="text-13 text-secondary">{t("activity.noOperations")}</p>}
      {batches.length > 0 && (
        <ul className="flex flex-col gap-2">
          {batches.map((batch) => (
            <BatchRow key={batch.id} account={account} batch={batch} mailboxes={mailboxes.data} />
          ))}
        </ul>
      )}
      {operations.hasNextPage && (
        <div>
          <Button
            variant="secondary"
            disabled={operations.isFetchingNextPage}
            onClick={() => {
              void operations.fetchNextPage();
            }}
          >
            {t("activity.loadMore")}
          </Button>
        </div>
      )}
    </section>
  );
}

/**
 * The activity view (stage 4): per account, what is on its way to the server, what failed and
 * what was changed elsewhere meanwhile, with "Try again" and "Remove from list". A view of its
 * own, not a dialog; kept current by `operationsChanged`.
 */
export function ActivityView() {
  const { t } = useTranslation();
  const dragRegion = useWindowDragRegion();
  const accounts = useAccounts();
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const list = accounts.data ?? [];
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewHeader title={t("activity.title")} subtitle={t("activity.subtitle")} dragRegion={dragRegion} />
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6" data-testid="activity-view">
        {accounts.isError && (
          <ErrorNotice
            context={t("settings.accounts.loadFailed")}
            error={accounts.error}
            onRetry={() => {
              void accounts.refetch();
            }}
          />
        )}
        {accounts.isSuccess && list.length === 0 ? (
          // Stage 8: without an account the next step is to add one.
          <EmptyState
            icon={History}
            title={t("activity.empty.title")}
            body={t("presentation.activity.noAccount")}
            action={{
              label: t("mail.noAccount.action"),
              onClick: () => {
                showAccountsPage({ kind: "add" });
              },
            }}
          />
        ) : (
          <div className="flex max-w-(--ui-content-max-width) flex-col gap-8">
            {list.map((account) => (
              <AccountActivity key={account.id} account={account} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
