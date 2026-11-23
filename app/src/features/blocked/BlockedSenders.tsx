import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import type { BlockedSender } from "../../services/ipc";
import { useBlockedSenders, useSetSenderBlocked } from "./queries";

/**
 * Stage 16, Settings → Inbox: the blocked senders (synced between devices). New mail from them
 * goes to the junk folder where the account may write, else it is hidden from the inbox lists;
 * unblocking shows hidden mail again. Senders are blocked from a message ("Block sender").
 */
export function BlockedSenders() {
  const { t, i18n } = useTranslation();
  const list = useBlockedSenders();
  const unblock = useSetSenderBlocked();
  const dates = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" });
  return (
    <SettingsSection title={t("blocked.title")}>
      <p className="text-13 text-secondary">{t("blocked.description")}</p>
      {list.isError && (
        <ErrorNotice
          context={t("blocked.loadFailed")}
          error={list.error}
          onRetry={() => {
            void list.refetch();
          }}
          data-testid="blocked-load-error"
        />
      )}
      {list.data?.length === 0 && (
        <p className="text-13 text-secondary" data-testid="blocked-empty">
          {t("blocked.empty")}
        </p>
      )}
      {list.data && list.data.length > 0 && (
        <ul
          aria-label={t("blocked.listLabel")}
          className="flex flex-col divide-y divide-border rounded-card border border-border"
          data-testid="blocked-senders"
        >
          {list.data.map((sender) => (
            <BlockedRow
              key={sender.address}
              sender={sender}
              since={dates.format(sender.createdAt)}
              disabled={unblock.isPending}
              onUnblock={() => {
                unblock.mutate({ address: sender.address, blocked: false });
              }}
            />
          ))}
        </ul>
      )}
      {unblock.isError && (
        <ErrorNotice context={t("blocked.unblockFailed")} error={unblock.error} data-testid="blocked-unblock-error" />
      )}
    </SettingsSection>
  );
}

function BlockedRow({ sender, since, disabled, onUnblock }: { sender: BlockedSender; since: string; disabled: boolean; onUnblock: () => void }) {
  const { t } = useTranslation();
  return (
    <li className="flex items-center gap-3 px-4 py-2" data-testid="blocked-sender">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-14 text-primary">{sender.address}</span>
        <span className="text-12 text-secondary">{t("blocked.since", { date: since })}</span>
      </div>
      <Button variant="secondary" disabled={disabled} onClick={onUnblock} aria-label={t("blocked.unblockLabel", { address: sender.address })}>
        {t("blocked.unblock")}
      </Button>
    </li>
  );
}
