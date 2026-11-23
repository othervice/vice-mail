import { AlarmClockCheck, BookOpen, Clock, ListTodo, Pin, SquareCheckBig, Zap, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { MessageListItem } from "../../services/ipc";

export type RowMark = "important" | "pinned" | "snoozed" | "returned" | "task" | "taskDone" | "readingList";

const MARK_ICONS: Record<RowMark, LucideIcon> = {
  important: Zap,
  pinned: Pin,
  snoozed: Clock,
  returned: AlarmClockCheck,
  task: ListTodo,
  taskDone: SquareCheckBig,
  readingList: BookOpen,
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
  return marks;
}

/** The marks as words for the row's accessible name. */
export function useRowMarkLabels(): (item: MessageListItem) => string[] {
  const { t } = useTranslation();
  return (item) =>
    rowMarks(item).map((mark) => (mark === "readingList" ? t("organize.readingList.mark") : t(`comfort.marks.${mark}`)));
}

/**
 * Small symbols next to the flag: important sender, pinned, snoozed ("back" in the accent when it
 * returned), task. Hidden from screen readers; the row's name says the same.
 */
export function RowMarks({ item, omit }: { item: MessageListItem; omit?: readonly RowMark[] | undefined }) {
  const { t } = useTranslation();
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
        const title = mark === "readingList" ? t("organize.readingList.mark") : t(`comfort.marks.${mark}`);
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
