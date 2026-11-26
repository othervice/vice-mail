import {
  AlarmClockCheck,
  BookOpen,
  Clock,
  ListTodo,
  MailClock,
  MailWarning,
  Pin,
  ShieldAlert,
  ShieldX,
  SquareCheckBig,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { MessageListItem } from "../../services/ipc";

export type RowMark =
  | "important"
  | "pinned"
  | "snoozed"
  | "returned"
  | "task"
  | "taskDone"
  | "readingList"
  | "followUp"
  | "noReply"
  | "spamSuspected"
  | "spamMoved";

const MARK_ICONS: Record<RowMark, LucideIcon> = {
  important: Zap,
  pinned: Pin,
  snoozed: Clock,
  returned: AlarmClockCheck,
  task: ListTodo,
  taskDone: SquareCheckBig,
  readingList: BookOpen,
  followUp: MailClock,
  noReply: MailWarning,
  spamSuspected: ShieldAlert,
  spamMoved: ShieldX,
};

/** Stage 9 marks of a row, in reading order: important, pinned, snoozed or back, task. */
export function rowMarks(item: MessageListItem): RowMark[] {
  const marks: RowMark[] = [];
  if (item.important) marks.push("important");
  if (item.pinned) marks.push("pinned");
  if (item.snooze) marks.push(item.snooze.returned ? "returned" : "snoozed");
  if (item.task) marks.push(item.task.done ? "taskDone" : "task");
  // Stage 16: on the reading list (local, never on the server).
  if (item.readingList) marks.push("readingList");
  // Stage 20 (decision 0059): a reminder if no reply comes, or none came by its time.
  if (item.followUp) marks.push(item.followUp.returned ? "noReply" : "followUp");
  // Stage 22 (decision 0077): the learning spam filter suspects it (marked on this device), or
  // moved it to the junk folder (shown only there).
  if (item.spam === "suspected") marks.push("spamSuspected");
  else if (item.spam === "moved") marks.push("spamMoved");
  return marks;
}

/** "23 Nov" (with the year when it is not this year's): the day a reminded message was sent. */
function sentDay(at: number, language: string): string {
  const withYear = new Date(at).getFullYear() !== new Date().getFullYear();
  return new Intl.DateTimeFormat(language, { day: "numeric", month: "short", ...(withYear && { year: "numeric" }) }).format(at);
}

function markLabel(mark: RowMark, item: MessageListItem, t: TFunction, language: string): string {
  if (mark === "readingList") return t("organize.readingList.mark");
  if (mark === "spamSuspected") return t("spam.marks.suspected");
  if (mark === "spamMoved") return t("spam.marks.moved");
  if (mark === "noReply" && item.followUp) return t("comfort.marks.noReplySince", { date: sentDay(item.followUp.sentAt, language) });
  return t(`comfort.marks.${mark}`);
}

/** The marks as words for the row's accessible name. */
export function useRowMarkLabels(): (item: MessageListItem) => string[] {
  const { t, i18n } = useTranslation();
  return (item) => rowMarks(item).map((mark) => markLabel(mark, item, t, i18n.language));
}

/**
 * Small symbols next to the flag: important sender, pinned, snoozed ("back" in the accent when it
 * returned), task. Hidden from screen readers; the row's name says the same.
 */
export function RowMarks({ item, omit }: { item: MessageListItem; omit?: readonly RowMark[] | undefined }) {
  const { t, i18n } = useTranslation();
  const marks = rowMarks(item).filter((mark) => !omit?.includes(mark));
  if (marks.length === 0) return null;
  return (
    <>
      {marks.map((mark) => {
        const Icon = MARK_ICONS[mark];
        if (mark === "returned") {
          return (
            <span
              key={mark}
              title={t("comfort.marks.returned")}
              data-testid="row-mark-returned"
              className="flex shrink-0 items-center gap-1 rounded-full bg-accent-soft px-2 text-12 font-semibold text-accent"
            >
              <Icon aria-hidden className="size-3" strokeWidth={2} />
              <span aria-hidden>{t("comfort.marks.back")}</span>
            </span>
          );
        }
        if (mark === "spamSuspected" || mark === "spamMoved") {
          // Stage 22: a word, not only a symbol: why the mail looks like spam, or why it is here.
          return (
            <span
              key={mark}
              title={markLabel(mark, item, t, i18n.language)}
              data-testid={`row-mark-${mark}`}
              className={
                mark === "spamSuspected"
                  ? "flex shrink-0 items-center gap-1 rounded-full border border-border px-2 text-12 font-semibold text-warning"
                  : "flex shrink-0 items-center gap-1 rounded-full border border-border px-2 text-12 font-medium text-secondary"
              }
            >
              <Icon aria-hidden className="size-3" strokeWidth={2} />
              <span aria-hidden>{t(mark === "spamSuspected" ? "spam.marks.suspectedShort" : "spam.marks.movedShort")}</span>
            </span>
          );
        }
        if (mark === "noReply") {
          // Stage 20: like "Back", with the day it was sent.
          return (
            <span
              key={mark}
              title={markLabel(mark, item, t, i18n.language)}
              data-testid="row-mark-noReply"
              className="flex shrink-0 items-center gap-1 rounded-full bg-accent-soft px-2 text-12 font-semibold text-accent"
            >
              <Icon aria-hidden className="size-3" strokeWidth={2} />
              <span aria-hidden>{markLabel(mark, item, t, i18n.language)}</span>
            </span>
          );
        }
        const title = markLabel(mark, item, t, i18n.language);
        return (
          <span key={mark} title={title} className="flex shrink-0" data-testid={`row-mark-${mark}`}>
            <Icon
              aria-hidden
              className={mark === "important" ? "size-(--ui-icon-small) text-warning" : "size-(--ui-icon-small) text-tertiary"}
              strokeWidth={1.75}
            />
          </span>
        );
      })}
    </>
  );
}
