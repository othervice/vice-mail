import { MessagesSquare } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { useSearchStore } from "../../stores/search";

/** Header switch between one row per conversation and one row per message (stage 7). */
export function ConversationToggle() {
  const { t } = useTranslation();
  const on = useSearchStore((state) => state.conversations);
  const setConversations = useSearchStore((state) => state.setConversations);
  const label = on ? t("threads.toggleOff") : t("threads.toggleOn");
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-pressed={on}
        aria-label={t("threads.toggle")}
        onClick={() => {
          setConversations(!on);
        }}
        className={cx(
          "flex size-8 items-center justify-center rounded-control transition-colors",
          on ? "bg-selected text-primary" : "text-secondary hover:bg-hover hover:text-primary",
        )}
        data-testid="conversation-toggle"
      >
        <MessagesSquare aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
