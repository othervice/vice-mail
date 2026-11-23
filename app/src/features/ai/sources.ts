import { useEffect, useRef } from "react";
import type { MessageId } from "../../services/ipc";
import { useAiPanelStore } from "../../stores/aiPanel";

/**
 * Stage 13: a click on a source of an AI statement shows that mail where the thread is open.
 * `show` runs for requests made after mounting, and only for messages of `ids`.
 */
export function useAiSourceFocus(ids: readonly MessageId[], show: (messageId: MessageId) => void): void {
  const request = useAiPanelStore((state) => state.sourceRequest);
  const handled = useRef(request?.seq ?? 0);
  useEffect(() => {
    if (!request || request.seq <= handled.current) return;
    handled.current = request.seq;
    if (ids.includes(request.messageId)) show(request.messageId);
  }, [request, ids, show]);
}
