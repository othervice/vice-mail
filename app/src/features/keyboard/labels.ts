import { useTranslation } from "react-i18next";
import { isViewCommand, VIEW_COMMANDS, type ActionId, type ActionLabel } from "../actions/registry";
import { isActionId, type CommandId } from "./bindings";

type ToggleId = Exclude<ActionId, ActionLabel>;

const TOGGLES: Record<ToggleId, true> = {
  toggleRead: true,
  toggleFlag: true,
  togglePin: true,
  toggleTask: true,
  toggleTaskDone: true,
};

const isToggle = (id: ActionId): id is ToggleId => id in TOGGLES;

/** A command's name without a context (settings, overview): toggles name both of their forms. */
export function useCommandLabel(): (id: CommandId) => string {
  const { t } = useTranslation();
  return (id) => {
    if (id === "search") return t("search.open");
    if (isViewCommand(id)) return t("palette.goTo", { name: t(`nav.${VIEW_COMMANDS[id]}`) });
    if (!isActionId(id)) return t(`keyboard.commands.${id}`);
    if (isToggle(id)) return t(`keyboard.toggles.${id}`);
    return t(`actions.labels.${id}`);
  };
}
