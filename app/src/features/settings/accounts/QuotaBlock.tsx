import { HardDrive, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { Notice } from "../../../components/Notice";
import type { Account } from "../../../services/ipc";
import { useSearchStore } from "../../../stores/search";
import { useUiStore } from "../../../stores/ui";
import { useAccountQuota, useRefreshQuota } from "../../accounts/quota";
import { formatSize } from "../../reading/format";

/** The search that lists the largest messages first in the account ("larger:10M"). */
export const LARGE_MAIL_QUERY = "larger:10M";

/** Opens the search over the account's mail for large messages (the way to make room). */
export function findLargeMessages(account: Account): void {
  const ui = useUiStore.getState();
  ui.selectAccount(account.id);
  ui.showMailView("inbox");
  const search = useSearchStore.getState();
  search.setScope("account");
  search.setQuery(LARGE_MAIL_QUERY);
  search.openSearch();
  search.submit();
}

/**
 * Stage 20 (decision 0060): storage of the mailbox on the server (IMAP QUOTA, JMAP quotas): used
 * and available with a bar, a notice from 90 % on with the way to make room; what a server does
 * not report is said as such.
 */
export function QuotaBlock({ account }: { account: Account }) {
  const { t, i18n } = useTranslation();
  const quota = useAccountQuota(account.id);
  const refresh = useRefreshQuota(account.id);
  const data = refresh.data ?? quota.data;
  const checking = quota.isPending || refresh.isPending;

  let body;
  if (checking && !data) {
    body = <p className="text-13 text-secondary">{t("settings.accounts.detail.quota.checking")}</p>;
  } else if (quota.isError) {
    body = <ErrorNotice context={t("settings.accounts.detail.quota.failed")} error={quota.error} />;
  } else if (data?.kind === "reported") {
    const used = formatSize(data.usedBytes, i18n.language);
    const limit = formatSize(data.limitBytes, i18n.language);
    const free = formatSize(Math.max(0, data.limitBytes - data.usedBytes), i18n.language);
    body = (
      <div className="flex flex-col gap-2" data-testid="account-quota" data-percent={data.percent}>
        <p className="text-14 text-primary">{t("settings.accounts.detail.quota.used", { used, limit, percent: data.percent })}</p>
        <meter
          min={0}
          max={100}
          low={75}
          high={90}
          optimum={0}
          value={data.percent}
          aria-label={t("settings.accounts.detail.quota.meter", { percent: data.percent })}
          className="h-2 w-full max-w-(--ui-meter-width)"
        />
        <p className="text-13 text-secondary">{t("settings.accounts.detail.quota.free", { free })}</p>
        {data.nearlyFull && (
          <Notice tone="warning" title={t("settings.accounts.detail.quota.nearlyFull", { percent: data.percent })} data-testid="account-quota-warning">
            <p>{t("settings.accounts.detail.quota.cleanUp")}</p>
            <div className="pt-2">
              <Button
                variant="secondary"
                onClick={() => {
                  findLargeMessages(account);
                }}
                data-testid="account-quota-find-large"
              >
                <Search aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
                {t("settings.accounts.detail.quota.findLarge")}
              </Button>
            </div>
          </Notice>
        )}
      </div>
    );
  } else if (data?.kind === "notReported") {
    body = (
      <p className="text-14 text-secondary" data-testid="account-quota-none">
        {t("settings.accounts.detail.quota.notReported")}
      </p>
    );
  } else if (data?.kind === "unknown") {
    body = data.error ? (
      <ErrorNotice context={t("settings.accounts.detail.quota.unknown")} error={data.error} />
    ) : (
      <p className="text-14 text-secondary">{t("settings.accounts.detail.quota.unknown")}</p>
    );
  }

  return (
    <section className="flex flex-col gap-3 border-t border-border pt-6" data-testid="account-quota-block">
      <div className="flex items-center gap-2">
        <HardDrive aria-hidden className="size-(--ui-icon-small) text-secondary" strokeWidth={1.75} />
        <h3 className="text-14 font-semibold text-primary">{t("settings.accounts.detail.quota.title")}</h3>
      </div>
      {body}
      {/* Stage 21 (decision 0071): alone under the text, the quiet button starts on its edge. */}
      <div className="-ms-3">
        <Button
          variant="quiet"
          disabled={checking}
          onClick={() => {
            refresh.mutate();
          }}
          data-testid="account-quota-refresh"
        >
          {refresh.isPending ? t("settings.accounts.detail.quota.checking") : t("settings.accounts.detail.quota.refresh")}
        </Button>
      </div>
      {refresh.isError && <ErrorNotice context={t("settings.accounts.detail.quota.failed")} error={refresh.error} />}
    </section>
  );
}
