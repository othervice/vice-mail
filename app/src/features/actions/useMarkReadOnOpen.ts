import { useEffect, useEffectEvent, useRef } from "react";
import { MARK_READ_DELAY_MS, type Account, type MessageId, type MessageListItem } from "../../services/ipc";
import { useReadingPreferences } from "./queries";
import type { ActionController } from "./useActionController";

/**
 * "Mark as read when opened" (Settings, Reading): right away, after `MARK_READ_DELAY_MS` of the
 * message staying open (a change of selection before cancels it), or never. Only for accounts
 * with writes on, and once per opening: a message the user marks unread while it is open stays
 * unread. An ordinary `markRead` action, without an undo notice.
 */
export function useMarkReadOnOpen(account: Account, item: MessageListItem | null, controller: ActionController) {
  const preferences = useReadingPreferences();
  const mode = preferences.data?.markReadOnOpen;
  const handled = useRef<MessageId | null>(null);
  const id = item?.id ?? null;
  const unread = item?.unread ?? false;
  const writes = account.writesEnabled;
  const markRead = useEffectEvent(() => {
    if (item) void controller.apply({ type: "markRead" }, [item], { silent: true, errorContext: "markRead" });
  });
  useEffect(() => {
    if (id === null) {
      handled.current = null;
      return undefined;
    }
    if (handled.current === id) return undefined;
    if (!unread) {
      handled.current = id;
      return undefined;
    }
    if (!mode || mode === "never" || !writes) return undefined;
    if (mode === "immediately") {
      handled.current = id;
      markRead();
      return undefined;
    }
    const timer = window.setTimeout(() => {
      handled.current = id;
      markRead();
    }, MARK_READ_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [id, unread, mode, writes]);
}
