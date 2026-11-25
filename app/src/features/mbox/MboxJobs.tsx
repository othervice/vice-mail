import { Download, FolderInput, Upload, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { mboxIpc, toCoreError, type Account, type CoreError, type CoreEvent, type Mailbox, type MboxJob } from "../../services/ipc";
import { useMboxStore } from "../../stores/mbox";
import { useNoticeStore } from "../../stores/notices";
import type { OverflowItem } from "../actions/OverflowMenu";

/** Stage 19 (decision 0057): `mboxJobChanged` from the core, in every window. */
export function handleMboxEvent(event: CoreEvent): void {
  if (event.type === "mboxJobChanged") useMboxStore.getState().upsert(event.job);
}

/**
 * "Export as mbox…" and "Import mbox…" for a folder's menu. Importing writes on the server: on
 * an account that only reads it stays in the menu and says why it does nothing.
 */
export function useMboxItems(account: Account | undefined, mailbox: Mailbox): { items: OverflowItem[] } {
  const { t } = useTranslation();
  const push = useNoticeStore((state) => state.push);
  const upsert = useMboxStore((state) => state.upsert);
  if (!account || !mailbox.selectable) return { items: [] };
  const started = (promise: ReturnType<typeof mboxIpc.exportFolder>) => {
    promise.then(
      (outcome) => {
        if (outcome.kind === "started") upsert(outcome.job);
      },
      (e: unknown) => {
        const failure = toCoreError(e);
        if (failure.code === "read_only") push({ kind: "readOnly", accountId: account.id });
        else push({ kind: "error", context: "mbox", error: failure });
      },
    );
  };
  const readOnly = !account.writesEnabled;
  return {
    items: [
      {
        id: "mbox-export",
        label: t("mbox.export"),
        icon: Download,
        onSelect: () => {
          started(mboxIpc.exportFolder(mailbox.id));
        },
        "data-testid": "folder-mbox-export",
      },
      {
        id: "mbox-import",
        label: readOnly ? t("actions.readOnly.tooltip", { label: t("mbox.import") }) : t("mbox.import"),
        icon: Upload,
        ...(readOnly && { inert: true }),
        onSelect: () => {
          if (readOnly) push({ kind: "readOnly", accountId: account.id });
          else started(mboxIpc.importIntoFolder(mailbox.id));
        },
        "data-testid": "folder-mbox-import",
      },
    ],
  };
}

function JobNotice({ job }: { job: MboxJob }) {
  const { t } = useTranslation();
  const dismiss = useMboxStore((state) => state.dismiss);
  const upsert = useMboxStore((state) => state.upsert);
  const [cancelError, setCancelError] = useState<CoreError | null>(null);
  const running = job.state === "running";
  const total = job.total;
  const percent = total !== null && total > 0 ? Math.min(100, Math.round(((job.done + job.skipped) / total) * 100)) : null;
  const title = t(`mbox.${job.kind}State.${job.state}`, { folder: job.mailboxName, file: job.fileName });
  const progress =
    job.kind === "export"
      ? t("mbox.exportProgress", { done: job.done, total: total ?? job.done })
      : t("mbox.importProgress", { done: job.done, queued: job.queued });
  return (
    <div
      role={job.state === "failed" ? "alert" : "status"}
      className={cx(
        "pointer-events-auto flex w-full max-w-(--ui-notice-width) flex-col gap-2 rounded-card border bg-surface-raised px-4 py-3 shadow-floating",
        job.state === "failed" ? "border-danger" : "border-border",
      )}
      data-testid="mbox-job"
      data-job-state={job.state}
      data-job-kind={job.kind}
    >
      <div className="flex items-start gap-3">
        <FolderInput aria-hidden className="size-(--ui-icon) shrink-0 text-accent" strokeWidth={1.75} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <p className="text-13 font-medium text-primary">{title}</p>
          <p className="text-12 text-secondary" data-testid="mbox-job-progress">
            {progress}
            {job.skipped > 0 && ` ${t("mbox.skipped", { count: job.skipped })}`}
          </p>
          {running && (
            /* Native, so no inline style is needed; without a total it shows as indeterminate. */
            <progress
              aria-label={title}
              max={100}
              value={percent ?? undefined}
              className="h-1 w-full appearance-none overflow-hidden rounded-full bg-hover accent-accent [&::-moz-progress-bar]:bg-accent [&::-webkit-progress-bar]:bg-hover [&::-webkit-progress-value]:bg-accent"
            />
          )}
          {job.error && <ErrorNotice context={t("mbox.failed")} error={job.error} />}
          {cancelError && <ErrorNotice context={t("mbox.cancelFailed")} error={cancelError} />}
          {running && (
            <div>
              <Button
                variant="quiet"
                onClick={() => {
                  mboxIpc.cancel(job.id).then(upsert, (e: unknown) => {
                    setCancelError(toCoreError(e));
                  });
                }}
                data-testid="mbox-job-cancel"
              >
                {t("mbox.cancel")}
              </Button>
            </div>
          )}
        </div>
        {!running && (
          <button
            type="button"
            aria-label={t("mbox.dismiss")}
            onClick={() => {
              dismiss(job.id);
            }}
            className="flex size-6 shrink-0 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary"
          >
            <X aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          </button>
        )}
      </div>
    </div>
  );
}

/** The jobs of this window at the bottom left: progress, cancel, the outcome. */
export function MboxJobsHost() {
  const jobs = useMboxStore((state) => state.jobs);
  if (jobs.length === 0) return null;
  return (
    <div className="pointer-events-none fixed start-6 bottom-6 z-30 flex w-(--ui-notice-width) flex-col gap-2" data-testid="mbox-jobs">
      {jobs.map((job) => (
        <JobNotice key={job.id} job={job} />
      ))}
    </div>
  );
}
