import type { QueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { comfortIpc, contactsIpc, parityIpc, type BlockedSender } from "../../services/ipc";
import { blockedSendersQueryKey } from "../blocked/queries";
import { refreshContacts } from "../contacts/queries";
import type { ComfortNotice } from "../../stores/notices";
import { refreshComfortLists } from "./queries";
import { formatSnoozeTime } from "./snooze";

export interface ComfortNoticeText {
  text: string;
  detail: string | null;
  error: boolean;
  /** "Undo" of a snooze: brings the messages back at once. */
  undo: ((queryClient: QueryClient) => void) | null;
}

/** What a stage 9 notice says, in the UI's language. */
export function comfortNoticeText(t: TFunction, language: string, notice: ComfortNotice): ComfortNoticeText {
  switch (notice.type) {
    case "snoozed": {
      const { messageIds } = notice;
      return {
        text: t("comfort.notices.snoozed", {
          count: notice.count,
          time: formatSnoozeTime(notice.until, Date.now(), language),
        }),
        detail: null,
        error: false,
        undo: (queryClient) => {
          void comfortIpc
            .unsnoozeMessages([...messageIds])
            .catch(() => undefined)
            .finally(() => {
              refreshComfortLists(queryClient, null);
            });
        },
      };
    }
    // Stage 16: the reading list and blocked senders (texts in `organize.notices`).
    case "readingListAdded":
    case "readingListRemoved":
      return { text: t(`organize.notices.${notice.type}`, { count: notice.count }), detail: null, error: false, undo: null };
    case "sendersBlocked": {
      const addresses = [...notice.addresses];
      return {
        text: t("organize.notices.senderBlocked", { count: addresses.length, address: addresses[0] ?? "" }),
        detail: null,
        error: false,
        undo: (queryClient) => {
          void (async () => {
            let list: BlockedSender[] | null = null;
            for (const address of addresses) list = await parityIpc.unblockSender(address).catch(() => list);
            if (list) queryClient.setQueryData(blockedSendersQueryKey, list);
            else void queryClient.invalidateQueries({ queryKey: blockedSendersQueryKey });
            refreshComfortLists(queryClient, null);
          })();
        },
      };
    }
    // Stage 17 (decision 0050): "Mark as spam" on a contact.
    case "contactSpam": {
      const { email, undo } = notice;
      return {
        text:
          notice.moved > 0
            ? t("contacts.view.notices.spamMoved", { name: notice.name, count: notice.moved })
            : t("contacts.view.notices.spam", { name: notice.name }),
        detail: notice.kept > 0 ? t("contacts.view.notices.kept", { count: notice.kept }) : null,
        error: false,
        undo: (queryClient) => {
          void contactsIpc
            .undoContactSpam(email, undo)
            .catch(() => undefined)
            .finally(() => {
              refreshContacts(queryClient);
              void queryClient.invalidateQueries({ queryKey: blockedSendersQueryKey });
              refreshComfortLists(queryClient, null);
            });
        },
      };
    }
    case "contactNotSpam":
      return { text: t("contacts.view.notices.notSpam", { name: notice.name }), detail: null, error: false, undo: null };
    case "failed":
      if (notice.what === "readingList" || notice.what === "block") {
        return {
          text: t(notice.what === "block" ? "organize.notices.blockFailed" : "organize.notices.readingListFailed"),
          detail:
            notice.error.code === "invalid_argument"
              ? t(notice.what === "block" ? "organize.notices.noAddress" : "organize.notices.noMessageId")
              : t(`errors.codes.${notice.error.code}.title`),
          error: true,
          undo: null,
        };
      }
      return {
        text: t(`comfort.notices.failed.${notice.what}`),
        detail:
          notice.error.code === "invalid_argument" && notice.what !== "unsnooze"
            ? t("comfort.notices.noMessageId")
            : t(`errors.codes.${notice.error.code}.title`),
        error: true,
        undo: null,
      };
    case "ruleSkipped":
      return {
        text: t("comfort.notices.ruleSkipped", { name: notice.ruleName, count: notice.count }),
        detail: t(`comfort.rules.skipped.${notice.reason}`),
        error: true,
        undo: null,
      };
    default:
      return { text: t(`comfort.notices.${notice.type}`, { count: notice.count }), detail: null, error: false, undo: null };
  }
}
