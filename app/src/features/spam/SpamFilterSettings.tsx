import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { Switch } from "../../components/Switch";
import {
  spamIpc,
  toCoreError,
  type AccountId,
  type CoreError,
  type SpamFilterStatus,
  type SpamLearningState,
} from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
import { useForgetSpamFilter, useLearnSpamFromMail, useSetSpamFilter, useSpamFilter } from "./queries";

/**
 * Stage 22 (decision 0077), Settings → Inbox: the learning spam filter. Off by default. On, it
 * learns from "Spam" and "Not spam" and looks at new inbox mail while the app runs; it can learn
 * from the junk folders and read inbox mail at once, with progress. Per account it can be left
 * off. What it learned stays on this device and can be forgotten.
 */
export function SpamFilterSettings() {
  const { t } = useTranslation();
  const status = useSpamFilter();
  const save = useSetSpamFilter();
  const learn = useLearnSpamFromMail();
  const [cancelError, setCancelError] = useState<CoreError | null>(null);
  const data = status.data;
  const settings = data?.settings ?? { enabled: false, excludedAccounts: [] };
  return (
    <SettingsSection sub title={t("spam.settings.title")}>
      <div className="flex flex-col gap-4" data-testid="spam-settings">
        {status.isError && (
          <ErrorNotice
            context={t("spam.settings.loadFailed")}
            error={status.error}
            onRetry={() => {
              void status.refetch();
            }}
          />
        )}
        <Switch
          label={t("spam.settings.enabled.label")}
          description={t("spam.settings.enabled.description")}
          checked={settings.enabled}
          disabled={!data || save.isPending}
          onChange={(enabled) => {
            save.mutate({ ...settings, enabled });
          }}
          data-testid="spam-enabled"
        />
        {save.isError && <ErrorNotice context={t("spam.settings.saveFailed")} error={save.error} />}
        {data && settings.enabled && (
          <>
            <Learned status={data} />
            <Learning
              status={data}
              starting={learn.isPending}
              onStart={() => {
                setCancelError(null);
                learn.mutate();
              }}
              onCancel={() => {
                spamIpc.cancelLearning().catch((e: unknown) => {
                  setCancelError(toCoreError(e));
                });
              }}
            />
            {learn.isError && <ErrorNotice context={t("spam.settings.learnFailed")} error={learn.error} />}
            {cancelError && <ErrorNotice context={t("spam.settings.cancelFailed")} error={cancelError} />}
            <AccountChoice
              excluded={settings.excludedAccounts ?? []}
              disabled={save.isPending}
              onChange={(excludedAccounts) => {
                save.mutate({ ...settings, excludedAccounts });
              }}
            />
          </>
        )}
        {data && (data.learnedSpam > 0 || data.learnedHam > 0) && <Forget />}
      </div>
    </SettingsSection>
  );
}

/** What it learned, and whether it acts yet. */
function Learned({ status }: { status: SpamFilterStatus }) {
  const { t, i18n } = useTranslation();
  const number = new Intl.NumberFormat(i18n.language);
  return (
    <div className="flex flex-col gap-1" data-testid="spam-learned">
      <p className="text-13 text-primary">
        {t("spam.settings.learned", {
          spam: number.format(status.learnedSpam),
          ham: number.format(status.learnedHam),
        })}
      </p>
      <p className="text-13 text-secondary" data-testid="spam-ready">
        {status.ready
          ? t("spam.settings.ready")
          : t("spam.settings.notReady", {
              spam: number.format(status.minSpam),
              ham: number.format(status.minHam),
            })}
      </p>
    </div>
  );
}

/** How a run of learning ended (`running` shows the progress instead). */
const ENDED = {
  running: "spam.settings.learn.done",
  done: "spam.settings.learn.done",
  cancelled: "spam.settings.learn.cancelled",
  failed: "spam.settings.learn.failed",
} as const satisfies Record<SpamLearningState, string>;

/** "Learn from existing mail", its progress and how it ended. */
function Learning({
  status,
  starting,
  onStart,
  onCancel,
}: {
  status: SpamFilterStatus;
  starting: boolean;
  onStart: () => void;
  onCancel: () => void;
}) {
  const { t, i18n } = useTranslation();
  const number = new Intl.NumberFormat(i18n.language);
  const learning = status.learning;
  const running = learning?.state === "running";
  const percent = learning && learning.total > 0 ? Math.round((learning.done / learning.total) * 100) : null;
  return (
    <div className="flex flex-col gap-2 rounded-card border border-border-subtle px-4 py-3" data-testid="spam-learning">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="text-14 font-medium text-primary">{t("spam.settings.learn.label")}</span>
          <span className="text-13 text-secondary">{t("spam.settings.learn.description")}</span>
        </div>
        {running ? (
          <Button variant="secondary" onClick={onCancel} data-testid="spam-learning-cancel">
            {t("spam.settings.learn.cancel")}
          </Button>
        ) : (
          <Button variant="secondary" disabled={starting} onClick={onStart} data-testid="spam-learn">
            {t("spam.settings.learn.start")}
          </Button>
        )}
      </div>
      {learning && (
        <div className="flex flex-col gap-2" role="status" data-testid="spam-learning-state" data-state={learning.state}>
          <p className="text-13 text-secondary">
            {running
              ? t("spam.settings.learn.running", { done: number.format(learning.done), total: number.format(learning.total) })
              : t(ENDED[learning.state], {
                  spam: number.format(learning.spam),
                  ham: number.format(learning.ham),
                })}
            {learning.skipped > 0 && ` ${t("spam.settings.learn.skipped", { count: learning.skipped })}`}
          </p>
          {running && (
            /* Native, so no inline style is needed; without a total it shows as indeterminate. */
            <progress
              aria-label={t("spam.settings.learn.label")}
              max={100}
              value={percent ?? undefined}
              className="h-1 w-full appearance-none overflow-hidden rounded-full bg-hover accent-accent [&::-moz-progress-bar]:bg-accent [&::-webkit-progress-bar]:bg-hover [&::-webkit-progress-value]:bg-accent"
            />
          )}
          {learning.error && <ErrorNotice context={t("spam.settings.learnFailed")} error={learning.error} />}
        </div>
      )}
    </div>
  );
}

/** Per account: whether the filter looks at its mail (all are on until the user leaves one out). */
function AccountChoice({
  excluded,
  disabled,
  onChange,
}: {
  excluded: readonly AccountId[];
  disabled: boolean;
  onChange: (excluded: AccountId[]) => void;
}) {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const list = accounts.data ?? [];
  if (list.length === 0) return null;
  return (
    <fieldset className="flex flex-col gap-2" data-testid="spam-accounts">
      <legend className="mb-2 text-14 font-medium text-primary">{t("spam.settings.accounts.label")}</legend>
      {list.map((account) => {
        const on = !excluded.includes(account.id);
        return (
          <label key={account.id} className="flex cursor-default items-start gap-2 text-13 text-primary">
            <input
              type="checkbox"
              checked={on}
              disabled={disabled}
              onChange={(event) => {
                const others = excluded.filter((id) => id !== account.id);
                onChange(event.target.checked ? others : [...others, account.id]);
              }}
              className="mt-0.5 size-4 shrink-0 accent-accent"
              data-testid={`spam-account-${account.email}`}
            />
            <span className="flex min-w-0 flex-col">
              <span className="truncate">{account.email}</span>
              {!account.writesEnabled && <span className="text-12 text-secondary">{t("spam.settings.accounts.readOnly")}</span>}
            </span>
          </label>
        );
      })}
    </fieldset>
  );
}

/** Forgets what was learned on this device, after a question. */
function Forget() {
  const { t } = useTranslation();
  const forget = useForgetSpamFilter();
  const [asking, setAsking] = useState(false);
  return (
    <div className="flex flex-col gap-2">
      <div>
        <Button
          variant="secondary"
          disabled={forget.isPending}
          onClick={() => {
            setAsking(true);
          }}
          data-testid="spam-forget"
        >
          {t("spam.settings.forget.button")}
        </Button>
      </div>
      {forget.isError && <ErrorNotice context={t("spam.settings.forget.failed")} error={forget.error} />}
      {asking && (
        <ConfirmDialog
          title={t("spam.settings.forget.title")}
          body={t("spam.settings.forget.body")}
          confirmLabel={t("spam.settings.forget.confirm")}
          cancelLabel={t("spam.settings.forget.cancel")}
          onConfirm={() => {
            setAsking(false);
            forget.mutate();
          }}
          onCancel={() => {
            setAsking(false);
          }}
          data-testid="spam-forget-dialog"
        />
      )}
    </div>
  );
}
