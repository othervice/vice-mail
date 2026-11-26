import { MessagesSquare } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ToolbarButton } from "../../components/ToolbarButton";
import { useConversationStore } from "./preference";

/**
 * Header switch between one row per conversation and one row per message (stage 7). Since
 * decision 0039 a stored UI preference, on by default and synced like the text size. Stage 23
 * (decision 0084): a button of the toolbar, over the list.
 */
export function ConversationToggle() {
  const { t } = useTranslation();
  const on = useConversationStore((state) => state.on);
  const setOn = useConversationStore((state) => state.setOn);
  const label = on ? t("threads.toggleOff") : t("threads.toggleOn");
  return (
    <ToolbarButton
      icon={MessagesSquare}
      label={t("threads.toggle")}
      hint={label}
      aria-pressed={on}
      active={on}
      onClick={() => {
        setOn(!on);
      }}
      // Narrow windows: in the list's "…" menu only (where it is too).
      className="@max-[70rem]/toolbar:hidden"
      data-testid="conversation-toggle"
    />
  );
}
