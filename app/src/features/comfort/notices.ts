import type { QueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { comfortIpc } from "../../services/ipc";
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
    case "failed":
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
