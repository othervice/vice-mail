import { Inbox, Search, SquarePen } from "lucide-react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { IconButton } from "../../components/IconButton";
import { ViewHeader } from "../../components/ViewHeader";
import { useUiStore, type MailViewId } from "../../stores/ui";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";

interface MailViewProps {
  viewId: MailViewId;
}

/**
 * List area of a mail view. Accounts arrive in stage 2; until then there is never an account, so
 * every view shows the same empty state with the next step instead of any list content.
 */
export function MailView({ viewId }: MailViewProps) {
  const { t } = useTranslation();
  const showSettings = useUiStore((state) => state.showSettings);
  const dragRegion = useWindowDragRegion();

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ViewHeader title={t(`nav.${viewId}`)} dragRegion={dragRegion}>
        <div className="flex items-center gap-1">
          <IconButton icon={Search} label={t("mail.search")} disabled />
          <IconButton icon={SquarePen} label={t("mail.compose")} disabled />
        </div>
      </ViewHeader>
      <EmptyState
        icon={Inbox}
        title={t("mail.noAccount.title")}
        body={t("mail.noAccount.body")}
        action={{
          label: t("mail.noAccount.action"),
          onClick: () => {
            showSettings("accounts");
          },
        }}
      />
    </div>
  );
}
