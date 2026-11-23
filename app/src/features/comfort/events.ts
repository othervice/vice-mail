import type { QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { comfortIpc, type CoreEvent, type NotificationTexts } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { useUiStore } from "../../stores/ui";
import { handleInboxChanged, refreshComfortListsOf } from "./queries";

/**
 * Stage 9 events: `inboxChanged` (pins, snoozes, tasks or categories changed, a snooze came
 * back) and `messagesChanged` reload the sorted inbox and the mark views; `ruleSkipped` says
 * which local rule could not act and why; a click on a system notification
 * (`openMessageRequested`) brings its folder to the front.
 */
export function handleComfortEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "inboxChanged":
      handleInboxChanged(queryClient, event.accountId);
      break;
    case "messagesChanged":
      refreshComfortListsOf(queryClient, event.accountId, event.mailboxId);
      break;
    case "ruleSkipped":
      useNoticeStore.getState().push({
        kind: "comfort",
        notice: { type: "ruleSkipped", ruleName: event.ruleName, reason: event.reason, count: event.count },
      });
      break;
    case "openMessageRequested": {
      const ui = useUiStore.getState();
      if (event.mailboxId !== null) ui.showMailbox(event.accountId, event.mailboxId);
      else {
        ui.selectAccount(event.accountId);
        ui.showMailView("inbox");
      }
      break;
    }
    default:
      break;
  }
}

/** The texts of system notifications in the UI's language, for `setNotificationTexts`. */
export function useNotificationTextValues(): NotificationTexts {
  const { t } = useTranslation();
  return {
    newMessage: t("comfort.notificationTexts.newMessage"),
    newMessages: t("comfort.notificationTexts.newMessages"),
    noSubject: t("comfort.notificationTexts.noSubject"),
    sendFailed: t("comfort.notificationTexts.sendFailed"),
    snoozeDue: t("comfort.notificationTexts.snoozeDue"),
  };
}

/**
 * Sends the notification texts to the core at start and after every language change (contract
 * of stage 9); until then the core uses English fallbacks. A failure only costs the translation,
 * so it is not shown.
 */
export function useNotificationTexts(): void {
  const { i18n } = useTranslation();
  const texts = useNotificationTextValues();
  const key = JSON.stringify(texts);
  useEffect(() => {
    void comfortIpc.setNotificationTexts(JSON.parse(key) as NotificationTexts).catch(() => undefined);
  }, [key, i18n.language]);
}
