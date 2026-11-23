import { MessagesSquare } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { Tooltip } from "../../components/Tooltip";
import { useConversationStore } from "./preference";

/**
 * Header switch between one row per conversation and one row per message (stage 7). Since
 * decision 0039 a stored UI preference, on by default and synced like the text size.
 */
export function ConversationToggle() {
  const { t } = useTranslation();
  const on = useConversationStore((state) => state.on);
  const setOn = useConversationStore((state) => state.setOn);
  const label = on ? t("threads.toggleOff") : t("threads.toggleOn");
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-pressed={on}
        aria-label={t("threads.toggle")}
        onClick={() => {
          setOn(!on);
        }}
        className={cx(
          "flex size-8 items-center justify-center rounded-control transition-colors focus-visible:outline-2 focus-visible:outline-focus-ring",
          on ? "bg-selected text-primary" : "text-secondary hover:bg-hover hover:text-primary",
        )}
        data-testid="conversation-toggle"
      >
        <MessagesSquare aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
      </button>
    </Tooltip>
  );
}
