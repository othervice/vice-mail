import { EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { AccountId, InboxScope } from "../../services/ipc";
import { useUiStore } from "../../stores/ui";
import { useBlockedHidden } from "../reading/attachmentQueries";
import { useBlockedFocusStore } from "./focus";

/**
 * Stage 16 (decision 0036): above an inbox list (one account's, or the unified inbox), a quiet
 * line when mail of blocked senders is left out of it, and the way to the list of blocked
 * senders. Nothing when none is hidden.
 */
export function BlockedHiddenNote({ scope, accountId }: { scope: InboxScope; accountId: AccountId | null }) {
  const { t } = useTranslation();
  const hidden = useBlockedHidden(scope, accountId);
  const showSettings = useUiStore((state) => state.showSettings);
  const requestFocus = useBlockedFocusStore((state) => state.request);
  if (hidden.isError) {
    return (
      <div className="shrink-0 px-4 py-2">
        <ErrorNotice
          context={t("blocked.loadFailed")}
          error={hidden.error}
          onRetry={() => {
            void hidden.refetch();
          }}
          data-testid="blocked-hidden-error"
        />
      </div>
    );
  }
  const count = hidden.data ?? 0;
  if (count <= 0) return null;
  return (
    <div
      role="note"
      className="flex shrink-0 items-center gap-2 border-b border-border-subtle bg-surface-list px-4 py-1"
      data-testid="blocked-hidden-note"
    >
      <EyeOff aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
      <p className="min-w-0 flex-1 truncate text-12 text-secondary">{t("attachments.blockedHidden.text", { count })}</p>
      <Button
        variant="quiet"
        aria-label={t("attachments.blockedHidden.manageLabel")}
        onClick={() => {
          requestFocus();
          showSettings("inbox");
        }}
        data-testid="blocked-hidden-manage"
      >
        {t("attachments.blockedHidden.manage")}
      </Button>
    </div>
  );
}
