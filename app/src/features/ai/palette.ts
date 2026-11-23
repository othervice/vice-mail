import { MessageCircleQuestionMark, Sparkles, SquarePen, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { currentActionTarget, useActionTargetStore } from "../../stores/actionTarget";
import { useAiPanelStore, type AiPanelCommand } from "../../stores/aiPanel";
import { useUiStore } from "../../stores/ui";
import type { PaletteEntry } from "../palette/entries";
import { panelAvailable } from "./AiPanel";
import { useAiAvailability } from "./queries";

const COMMANDS: readonly { kind: AiPanelCommand["kind"]; icon: LucideIcon; needs: "summary" | "replySuggestion" | "search" }[] = [
  { kind: "summarize", icon: Sparkles, needs: "summary" },
  { kind: "suggestReply", icon: SquarePen, needs: "replySuggestion" },
  { kind: "ask", icon: MessageCircleQuestionMark, needs: "search" },
];

/**
 * Stage 13 entries of the command palette, in the style of the action directory: they run in the
 * AI panel (so the request and its cancel button are visible) and only exist while the function
 * is usable. With AI off there are none, so the palette cannot start a request.
 */
export function useAiPaletteEntries(): PaletteEntry[] {
  const { t } = useTranslation();
  const availability = useAiAvailability().data;
  const view = useUiStore((state) => state.view);
  const showMailView = useUiStore((state) => state.showMailView);
  const ctx = useActionTargetStore((state) => currentActionTarget(state.targets)?.ctx ?? null);
  const open = useAiPanelStore((state) => state.open);
  const toggle = useAiPanelStore((state) => state.toggle);
  const request = useAiPanelStore((state) => state.request);
  if (!panelAvailable(availability) || !availability) return [];
  const inMail = view.kind === "mail" || view.kind === "mailbox";
  const single = ctx?.items.length === 1;
  const entries: PaletteEntry[] = [
    {
      key: "ai:panel",
      group: "commands",
      label: open ? t("ai.panel.hide") : t("ai.panel.show"),
      icon: Sparkles,
      run: () => {
        if (!inMail) showMailView("inbox");
        if (!open || inMail) toggle();
      },
    },
  ];
  for (const command of COMMANDS) {
    if (!availability[command.needs]) continue;
    const needsMessage = command.kind !== "ask";
    const disabled = !inMail ? t("palette.disabled.noView") : needsMessage && !single ? t("palette.disabled.noMessage") : undefined;
    entries.push({
      key: `ai:${command.kind}`,
      group: "commands",
      label: t(`ai.palette.${command.kind}`),
      icon: command.icon,
      ...(disabled && { disabled }),
      run: () => {
        request(command.kind);
      },
    });
  }
  return entries;
}
