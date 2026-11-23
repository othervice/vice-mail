import { UserX } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Address } from "../../services/ipc";
import { useBlockedSenders, useSetSenderBlocked } from "../blocked/queries";

/**
 * Stage 16: a quiet note on a message whose sender the user blocked (found in a folder, the search
 * or Junk): what blocking does, and the way back. Existing mail is not moved by blocking.
 */
export function BlockedNote({ from }: { from: readonly Address[] }) {
  const { t } = useTranslation();
  const blocked = useBlockedSenders();
  const change = useSetSenderBlocked();
  const address = from.map((entry) => entry.email.trim().toLowerCase()).find((email) => email !== "");
  if (!address || !blocked.data?.some((entry) => entry.address === address)) return null;
  return (
    <div
      role="note"
      data-testid="blocked-note"
      data-print-hide
      className="flex flex-col gap-2 rounded-card border border-border-subtle bg-canvas px-3 py-2"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <UserX aria-hidden className="size-(--ui-icon-small) shrink-0 text-secondary" strokeWidth={1.75} />
        <p className="min-w-0 flex-1 text-13 text-secondary">{t("organize.blockedNote.text", { address })}</p>
        <Button
          variant="quiet"
          disabled={change.isPending}
          onClick={() => {
            change.mutate({ address, blocked: false });
          }}
          data-testid="blocked-note-unblock"
        >
          {t("organize.blockedNote.unblock")}
        </Button>
      </div>
      {change.error && <ErrorNotice context={t("organize.notices.unblockFailed")} error={change.error} />}
    </div>
  );
}
