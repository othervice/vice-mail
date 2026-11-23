import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Switch } from "../../components/Switch";
import type { Account } from "../../services/ipc";
import { useHiddenAccounts, useSetAccountHidden } from "./queries";

/**
 * Settings of one account, stage 7: whether its mail shows in the unified inbox, in groups and
 * in "search in all accounts". On this device only; the account keeps syncing.
 */
export function HiddenAccountBlock({ account }: { account: Account }) {
  const { t } = useTranslation();
  const hidden = useHiddenAccounts();
  const save = useSetAccountHidden();
  const isHidden = save.isPending ? save.variables.hidden : (hidden.data?.includes(account.id) ?? false);
  return (
    <div className="flex flex-col gap-3" data-testid="hidden-account">
      {hidden.isError ? (
        <ErrorNotice
          context={t("hiddenAccount.loadFailed")}
          error={hidden.error}
          onRetry={() => {
            void hidden.refetch();
          }}
        />
      ) : (
        <Switch
          label={t("hiddenAccount.label")}
          description={t("hiddenAccount.description")}
          checked={!isHidden}
          disabled={hidden.isPending || save.isPending}
          onChange={(shown) => {
            save.mutate({ accountId: account.id, hidden: !shown });
          }}
          data-testid="hidden-account-switch"
        />
      )}
      {save.isError && <ErrorNotice context={t("hiddenAccount.failed")} error={save.error} />}
    </div>
  );
}
