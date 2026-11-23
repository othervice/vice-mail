import { useQuery } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { useEffect, useRef } from "react";
import { emlIpc, MAX_MESSAGES_BY_IDS, toCoreError, type MessageId, type MessageListItem } from "../../services/ipc";
import { currentActionTarget, useActionTargetStore } from "../../stores/actionTarget";
import { useAiPanelStore } from "../../stores/aiPanel";
import { useNoticeStore } from "../../stores/notices";
import { formatFullDate } from "../messages/rows";
import { openMessageInWindow } from "../windows/open";

/**
 * Stage 13: a click on a source of an AI statement shows that mail where the thread is open.
 * `show` runs for requests made after mounting, and only for messages of `ids`. While mounted,
 * the pane's messages count as shown (decision 0027: other sources open in a window).
 */
export function useAiSourceFocus(ids: readonly MessageId[], show: (messageId: MessageId) => void): void {
  const request = useAiPanelStore((state) => state.sourceRequest);
  const handled = useRef(request?.seq ?? 0);
  useEffect(() => {
    if (!request || request.seq <= handled.current) return;
    handled.current = request.seq;
    if (ids.includes(request.messageId)) show(request.messageId);
  }, [request, ids, show]);
  const key = ids.join(",");
  useEffect(() => {
    const shown = key === "" ? [] : key.split(",").map(Number);
    useAiPanelStore.getState().setShownSources(shown);
    return () => {
      useAiPanelStore.getState().setShownSources([]);
    };
  }, [key]);
}

/** "Grace Hopper, 20 Nov 2026": how a source is named. */
export function sourceLabel(t: TFunction, language: string, item: MessageListItem): string {
  const from = item.from;
  const name = from ? from.name || from.email : t("list.unknownSender");
  return t("ai.sources.label", { name, date: formatFullDate(item.date, language) });
}

/**
 * Decision 0027: the list rows of the sources of an answer, from the core by id (unknown ones
 * are missing; the panel then numbers them). At most `MAX_MESSAGES_BY_IDS`, in order.
 */
export function useSourceItems(ids: readonly MessageId[]): Map<MessageId, MessageListItem> {
  const unique = [...new Set(ids)].slice(0, MAX_MESSAGES_BY_IDS);
  const query = useQuery({
    queryKey: ["ai", "sources", unique],
    queryFn: () => emlIpc.getMessagesByIds(unique),
    enabled: unique.length > 0,
    staleTime: 30_000,
  });
  return new Map((query.data ?? []).map((item) => [item.id, item]));
}

/**
 * A click on a source: the main window shows it when it has the message (the selected one, or
 * one of the open thread, which then selects it); otherwise the message opens in a window of
 * its own. A message that is gone is a notice with the reason.
 */
export async function openSource(messageId: MessageId): Promise<void> {
  const panel = useAiPanelStore.getState();
  const items = currentActionTarget(useActionTargetStore.getState().targets)?.ctx?.items ?? [];
  const selected = items.length === 1 && items[0]?.id === messageId;
  if (selected || panel.shownSources.includes(messageId)) {
    panel.showSource(messageId);
    return;
  }
  try {
    const [item] = await emlIpc.getMessagesByIds([messageId]);
    if (item) {
      openMessageInWindow(item.accountId, item.id);
      return;
    }
    useNoticeStore.getState().push({
      kind: "error",
      context: "window",
      error: { code: "not_found", message: "The message is gone.", detail: null },
    });
  } catch (error) {
    useNoticeStore.getState().push({ kind: "error", context: "window", error: toCoreError(error) });
  }
}
