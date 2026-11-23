import { useId, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ChoiceGroup, type ChoiceOption } from "../../components/ChoiceGroup";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import type { Account, MarkReadOnOpen, PrefetchPeriod, PrefetchSettings } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { useAccounts } from "../accounts/queries";
import { useReadingPreferences, useSetReadingPreferences } from "../actions/queries";
import { formatSize } from "../reading/format";
import { useConversationStore } from "../threads/preference";
import {
  usePrefetchSettings,
  usePrefetchStatus,
  useRemoteSenders,
  useRevokeRemoteContent,
  useSetPrefetchSettings,
} from "../reading/queries";

const MIB = 1024 * 1024;

const PERIODS: readonly PrefetchPeriod[] = ["days7", "days30", "days90", "days365", "all"];

/** Size limits offered; 0 stores only the text of every message. The core default is 5 MiB. */
const SIZE_LIMITS: readonly number[] = [0, MIB, 5 * MIB, 25 * MIB, 100 * MIB];

interface BlockProps {
  title: string;
  description: string;
  /** A line above, to set it apart from the block before. */
  divided?: boolean;
  children: ReactNode;
}

function Block({ title, description, divided = false, children }: BlockProps) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className={cx("flex flex-col gap-4", divided && "border-t border-border pt-6")}>
      <div className="flex flex-col gap-1">
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {title}
        </h3>
        <p className="text-13 text-secondary">{description}</p>
      </div>
      {children}
    </section>
  );
}

function AccountName({ account }: { account: Account }) {
  const { t } = useTranslation();
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
      <span className="truncate text-13 font-medium text-primary">
        {account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email}
      </span>
    </span>
  );
}

function PrefetchStatusLine({ account }: { account: Account }) {
  const { t, i18n } = useTranslation();
  const status = usePrefetchStatus(account.id);
  let text: string | null = null;
  if (status.data) {
    const { state, done, total, bytes } = status.data;
    text =
      state === "running"
        ? t("settings.reading.prefetch.status.running", { done, total, size: formatSize(bytes, i18n.language) })
        : t(`settings.reading.prefetch.status.${state}`);
  }
  return (
    <li className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1" data-testid={`prefetch-status-${account.id}`}>
      <AccountName account={account} />
      {text && <span className="text-13 text-secondary">{text}</span>}
      {status.isError && <span className="text-13 text-danger">{t("settings.reading.prefetch.status.unknown")}</span>}
    </li>
  );
}

function PrefetchBlock({ accounts }: { accounts: readonly Account[] }) {
  const { t, i18n } = useTranslation();
  const settings = usePrefetchSettings();
  const save = useSetPrefetchSettings();
  const current = settings.data;

  const periods: ChoiceOption<PrefetchPeriod>[] = PERIODS.map((value) => ({
    value,
    label: t(`settings.reading.prefetch.period.${value}`),
  }));
  // A limit set elsewhere (another version of the app) stays selectable as it is.
  const limits = current && !SIZE_LIMITS.includes(current.maxMessageSize) ? [...SIZE_LIMITS, current.maxMessageSize] : SIZE_LIMITS;
  const sizes: ChoiceOption<string>[] = [...limits]
    .sort((a, b) => a - b)
    .map((bytes) => ({
      value: String(bytes),
      label: bytes === 0 ? t("settings.reading.prefetch.size.textOnly") : formatSize(bytes, i18n.language),
    }));
  const update = (next: Partial<PrefetchSettings>) => {
    if (current) save.mutate({ ...current, ...next });
  };

  return (
    <>
      {settings.isError && (
        <ErrorNotice
          context={t("settings.reading.prefetch.loadFailed")}
          error={settings.error}
          onRetry={() => {
            void settings.refetch();
          }}
        />
      )}
      {current && (
        <>
          <ChoiceGroup
            label={t("settings.reading.prefetch.period.label")}
            description={t("settings.reading.prefetch.period.description")}
            value={current.period}
            options={periods}
            onChange={(period) => {
              update({ period });
            }}
          />
          <ChoiceGroup
            label={t("settings.reading.prefetch.size.label")}
            description={t("settings.reading.prefetch.size.description")}
            value={String(current.maxMessageSize)}
            options={sizes}
            onChange={(value) => {
              update({ maxMessageSize: Number(value) });
            }}
          />
        </>
      )}
      {save.isError && <ErrorNotice context={t("errors.context.savePreferences")} error={save.error} />}
      {accounts.length > 0 && (
        <ul aria-label={t("settings.reading.prefetch.status.label")} className="flex flex-col gap-2">
          {accounts.map((account) => (
            <PrefetchStatusLine key={account.id} account={account} />
          ))}
        </ul>
      )}
    </>
  );
}

/**
 * Decision 0039: one row per conversation (the default) or per message, in every list that can
 * show conversations. A UI preference of the core, synced like the text size; the header's switch
 * and the list's menu change the same value.
 */
function ConversationsBlock() {
  const { t } = useTranslation();
  const on = useConversationStore((state) => state.on);
  const setOn = useConversationStore((state) => state.setOn);
  const saveError = useConversationStore((state) => state.saveError);
  const options: ChoiceOption<"on" | "off">[] = [
    { value: "on", label: t("threads.setting.on") },
    { value: "off", label: t("threads.setting.off") },
  ];
  return (
    <Block divided title={t("threads.setting.title")} description={t("threads.setting.description")}>
      <ChoiceGroup
        label={t("threads.setting.label")}
        value={on ? "on" : "off"}
        options={options}
        onChange={(value) => {
          setOn(value === "on");
        }}
      />
      {saveError && <ErrorNotice context={t("threads.setting.saveFailed")} error={saveError} />}
    </Block>
  );
}

const MARK_READ: readonly MarkReadOnOpen[] = ["immediately", "afterDelay", "never"];

/** When an opened message counts as read (stage 4). Only accounts with writes on are marked. */
function MarkReadBlock() {
  const { t } = useTranslation();
  const preferences = useReadingPreferences();
  const save = useSetReadingPreferences();
  const options: ChoiceOption<MarkReadOnOpen>[] = MARK_READ.map((value) => ({
    value,
    label: t(`settings.reading.markRead.${value}`),
  }));
  return (
    <Block title={t("settings.reading.markRead.title")} description={t("settings.reading.markRead.description")}>
      {preferences.isError && (
        <ErrorNotice
          context={t("settings.reading.markRead.loadFailed")}
          error={preferences.error}
          onRetry={() => {
            void preferences.refetch();
          }}
        />
      )}
      {preferences.data && (
        <ChoiceGroup
          label={t("settings.reading.markRead.label")}
          value={preferences.data.markReadOnOpen}
          options={options}
          onChange={(markReadOnOpen) => {
            save.mutate({ ...preferences.data, markReadOnOpen });
          }}
        />
      )}
      {save.isError && <ErrorNotice context={t("errors.context.savePreferences")} error={save.error} />}
    </Block>
  );
}

function SenderList({ account }: { account: Account }) {
  const { t } = useTranslation();
  const senders = useRemoteSenders(account.id);
  const revoke = useRevokeRemoteContent(account.id);
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-2" data-testid={`remote-senders-${account.id}`}>
      <h4 id={headingId}>
        <AccountName account={account} />
      </h4>
      {senders.isError && (
        <ErrorNotice
          context={t("settings.reading.remote.loadFailed")}
          error={senders.error}
          onRetry={() => {
            void senders.refetch();
          }}
        />
      )}
      {senders.data?.length === 0 && <p className="text-13 text-secondary">{t("settings.reading.remote.empty")}</p>}
      {senders.data && senders.data.length > 0 && (
        <ul className="flex flex-col gap-1">
          {senders.data.map((sender) => (
            <li key={sender} className="flex items-center justify-between gap-3 rounded-card px-3 py-1 hover:bg-hover">
              <span className="min-w-0 truncate text-13 text-primary select-text">{sender}</span>
              <Button
                variant="quiet"
                disabled={revoke.isPending}
                aria-label={t("settings.reading.remote.removeLabel", { sender })}
                onClick={() => {
                  revoke.mutate(sender);
                }}
                data-testid="revoke-sender"
              >
                {t("settings.reading.remote.remove")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {revoke.isError && <ErrorNotice context={t("settings.reading.remote.removeFailed")} error={revoke.error} />}
    </section>
  );
}

/** Settings → Reading: the read status, what is downloaded in advance, and whose external content loads. */
export function ReadingSettings() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const list = accounts.data ?? [];
  return (
    <SettingsSection title={t("settings.reading.title")}>
      <MarkReadBlock />
      <ConversationsBlock />
      <Block divided title={t("settings.reading.prefetch.title")} description={t("settings.reading.prefetch.description")}>
        <PrefetchBlock accounts={list} />
      </Block>
      <Block divided title={t("settings.reading.remote.title")} description={t("settings.reading.remote.description")}>
        {accounts.isError && (
          <ErrorNotice
            context={t("settings.accounts.loadFailed")}
            error={accounts.error}
            onRetry={() => {
              void accounts.refetch();
            }}
          />
        )}
        {accounts.isSuccess && list.length === 0 && <p className="text-13 text-secondary">{t("settings.reading.remote.noAccounts")}</p>}
        {list.map((account) => (
          <SenderList key={account.id} account={account} />
        ))}
      </Block>
    </SettingsSection>
  );
}
