import type { QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { comfortIpc, type CoreEvent, type NotificationTexts } from "../../services/ipc";
import { useAiPanelStore } from "../../stores/aiPanel";
import { selectWhenListed } from "../../stores/listSelect";
import { refreshAttachmentPins } from "../reading/attachmentQueries";
import { useNoticeStore } from "../../stores/notices";
import { useUiStore } from "../../stores/ui";
import { openMessageInWindow } from "../windows/open";
import { currentWindowRoute } from "../windows/route";
import { handleInboxChanged, refreshComfortListsOf } from "./queries";

/**
 * Stage 9 events: `inboxChanged` (pins, snoozes, tasks or categories changed, a snooze came
 * back) and `messagesChanged` reload the sorted inbox and the mark views; `ruleSkipped` says
 * which local rule could not act and why; a click on a system notification
 * (`openMessageRequested`) brings its folder to the front and opens the mail (decisions 0027,
 * 0028): in the thread pane when it shows the mail already, else selected in the folder's list
 * once that has loaded it, else in a window of its own (the list is paged; the mail may be on
 * no loaded page).
 */
export function handleComfortEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "inboxChanged":
      handleInboxChanged(queryClient, event.accountId);
      break;
    case "messagesChanged":
      refreshComfortListsOf(queryClient, event.accountId, event.mailboxId);
      // Stage 16 (decision 0036): a pinned attachment's message may be gone.
      refreshAttachmentPins(queryClient);
      break;
    case "ruleSkipped":
      useNoticeStore.getState().push({
        kind: "comfort",
        notice: { type: "ruleSkipped", ruleName: event.ruleName, reason: event.reason, count: event.count },
      });
      break;
    case "openMessageRequested": {
      // Every window hears the event; the main window answers it.
      if (currentWindowRoute().kind !== "main") break;
      const ui = useUiStore.getState();
      const view = ui.view;
      const sameFolder = view.kind === "mailbox" && event.mailboxId !== null && view.mailboxId === event.mailboxId;
      if (event.mailboxId !== null) ui.showMailbox(event.accountId, event.mailboxId);
      else {
        ui.selectAccount(event.accountId);
        ui.showMailView("inbox");
      }
      const panel = useAiPanelStore.getState();
      if (sameFolder && panel.shownSources.includes(event.messageId)) {
        panel.showSource(event.messageId);
        break;
      }
      const { accountId, mailboxId, messageId } = event;
      selectWhenListed({
        accountId,
        mailboxId,
        messageId,
        fallback: () => {
          openMessageInWindow(accountId, messageId);
        },
      });
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
