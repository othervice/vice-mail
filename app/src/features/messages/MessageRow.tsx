import { Check, Clock, Flag, Paperclip, TriangleExclamationPoint } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { HighlightedText } from "../../components/HighlightedText";
import type { AccountColor, Address, MessageActivity, MessageListItem, SearchHighlights } from "../../services/ipc";
import { Avatar } from "../accounts/Avatar";
import { RowMarks, useRowMarkLabels } from "../comfort/RowMarks";
import { formatFullDate, formatListDate, type DayBounds } from "./rows";

interface MessageRowContentProps {
  item: MessageListItem;
  accountColor: AccountColor;
  bounds: DayBounds;
  /** Sent and drafts show the recipients where other folders show the sender. */
  showRecipients: boolean;
  twoLine: boolean;
  /** Part of the selection. */
  selected: boolean;
  /** The keyboard cursor (`aria-activedescendant`): shows the focus ring while the list has focus. */
  cursor: boolean;
  /** More than one message is selected: every row shows its selection box. */
  multiSelect: boolean;
  /** Id for the preview text, so the option can point at it (`aria-describedby`). */
  previewId?: string | undefined;
  /** Quick actions, shown on hover and on the keyboard cursor (mouse shortcuts, see MessageList). */
  actions?: ReactNode;
  /** Stage 7: conversation size and search highlights. */
  decoration?: RowDecoration | undefined;
}

/** Stage 7: what a row of a conversation list or of search results adds to the message. */
export interface RowDecoration {
  /** Messages in the conversation; shown after the subject when more than one. */
  threadCount?: number;
  /** Found places (UTF-16 ranges) in subject, sender and preview. */
  highlights?: SearchHighlights;
  /** Stage 8: the account's name, in lists that mix accounts; shown with the colour. */
  account?: string;
}

/** Where `trimmed` starts in `raw`, so ranges into `raw` still fit after trimming. */
function leadingSpace(raw: string): number {
  return raw.length - raw.trimStart().length;
}

function addressName(address: Address): string {
  return address.name?.trim() || address.email;
}

/** Who a row names: the sender, or the recipients in Sent and Drafts. */
function useWho(item: MessageListItem, showRecipients: boolean): string {
  const { t } = useTranslation();
  if (showRecipients) {
    return item.to.length > 0 ? t("list.to", { names: item.to.map(addressName).join(", ") }) : t("list.unknownSender");
  }
  return item.from ? addressName(item.from) : t("list.unknownSender");
}

/** The row's activity, absent for most rows (the core leaves the field out). */
export function rowActivity(item: MessageListItem): MessageActivity | null {
  return item.activity ?? null;
}

/**
 * The row's accessible name, in reading order: state, who, subject, account (lists that mix
 * accounts), attachment, flag, pending or failed change, full date. Explicit, because a name
 * computed from the row's parts would run words together.
 */
export function useRowLabel(item: MessageListItem, showRecipients: boolean, decoration?: RowDecoration): string {
  const { t, i18n } = useTranslation();
  const who = useWho(item, showRecipients);
  const activity = rowActivity(item);
  const threadCount = decoration?.threadCount ?? 1;
  const comfortMarks = useRowMarkLabels();
  return [
    item.unread && t("list.unread"),
    who,
    item.subject.trim() || t("list.noSubject"),
    decoration?.account && t("presentation.rowAccount", { name: decoration.account }),
    threadCount > 1 && t("threads.rowLabel", { count: threadCount }),
    item.hasAttachments && t("list.attachment"),
    item.flagged && t("list.flagged"),
    activity && t(`list.activity.${activity}`),
    // Stage 9: important, pinned, snoozed or back, task.
    ...comfortMarks(item),
    formatFullDate(item.date, i18n.language),
  ]
    .filter(Boolean)
    .join(", ");
}

/** Small symbol for a change on its way (clock) or one that needs the user (warning). */
function ActivityIcon({ activity }: { activity: MessageActivity }) {
  const { t } = useTranslation();
  const label = t(`list.activity.${activity}`);
  const Icon = activity === "pending" ? Clock : TriangleExclamationPoint;
  return (
    <span title={label} className="flex shrink-0" data-testid={`row-activity-${activity}`}>
      <Icon
        aria-hidden
        className={cx(
          "size-(--ui-icon-small)",
          activity === "pending" ? "text-tertiary" : activity === "failed" ? "text-danger" : "text-warning",
        )}
        strokeWidth={1.75}
      />
    </span>
  );
}

/**
 * The unread dot, which turns into a selection box on hover, on selected rows and while several
 * messages are selected (docs/DESIGN.md, "Punkt links"). A mouse target only: the option's
 * `aria-selected` says the same to screen readers, and Space or X toggles it from the keyboard.
 */
function SelectionMark({ unread, selected, multiSelect }: { unread: boolean; selected: boolean; multiSelect: boolean }) {
  const boxShown = selected && multiSelect;
  return (
    <span aria-hidden data-row-check className="relative flex size-(--ui-icon-small) shrink-0 items-center justify-center">
      {unread && (
        <span
          className={cx(
            "size-(--ui-dot) rounded-full bg-accent",
            boxShown || multiSelect ? "hidden" : "group-hover/row:hidden",
          )}
          data-testid="row-unread-dot"
        />
      )}
      <span
        className={cx(
          "absolute inset-0 items-center justify-center rounded-control border",
          selected ? "border-accent bg-accent text-on-accent" : "border-secondary bg-surface-raised",
          boxShown || multiSelect ? "flex" : "hidden group-hover/row:flex",
        )}
        data-testid="row-check"
      >
        {selected && <Check className="size-3" strokeWidth={3} />}
      </span>
    </span>
  );
}

/**
 * One message in the list (docs/DESIGN.md, "Zeile"): unread dot (selection box on hover), avatar,
 * sender, subject with attachment symbol, preview, flag, pending or failed change, time, and the
 * quick actions on hover. The option around it carries the accessible name (`useRowLabel`); the
 * preview describes it.
 */
export function MessageRowContent({
  item,
  accountColor,
  bounds,
  showRecipients,
  twoLine,
  selected,
  cursor,
  multiSelect,
  previewId,
  actions,
  decoration,
}: MessageRowContentProps) {
  const { t, i18n } = useTranslation();
  const person = showRecipients ? item.to[0] : item.from;
  const who = useWho(item, showRecipients);
  const subject = item.subject.trim() || t("list.noSubject");
  const time = formatListDate(item.date, bounds, i18n.language);
  const activity = rowActivity(item);
  const highlights = decoration?.highlights;

  const sender = (
    <span
      title={who}
      className={cx(
        "truncate text-14 text-primary",
        item.unread ? "font-semibold" : "font-regular",
        !twoLine && "w-(--ui-list-sender-width) shrink-0",
        twoLine && "min-w-0 flex-1",
      )}
    >
      {highlights && !showRecipients && item.from ? (
        <HighlightedText text={who} ranges={highlights.from} offset={item.from.name?.trim() ? leadingSpace(item.from.name) : 0} />
      ) : (
        who
      )}
    </span>
  );
  const preview = item.preview?.trim();
  const threadCount = decoration?.threadCount ?? 1;
  // The subject comes first; the preview takes what is left of the line (basis 0), down to nothing.
  const subjectLine = (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span title={subject} className={cx("min-w-0 truncate text-14", item.subject.trim() ? "text-primary" : "text-tertiary")}>
        {highlights && item.subject.trim() ? (
          <HighlightedText text={subject} ranges={highlights.subject} offset={leadingSpace(item.subject)} />
        ) : (
          subject
        )}
      </span>
      {threadCount > 1 && (
        <span
          aria-hidden
          className="shrink-0 rounded-control border border-border px-1 text-12 text-secondary tabular-nums"
          data-testid="row-thread-count"
        >
          {new Intl.NumberFormat(i18n.language).format(threadCount)}
        </span>
      )}
      {item.hasAttachments && (
        <Paperclip aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      )}
      {preview && (
        <span id={previewId} className="min-w-0 flex-1 basis-0 truncate text-13 text-tertiary" data-testid="row-preview">
          {highlights ? (
            <HighlightedText text={preview} ranges={highlights.preview} offset={leadingSpace(item.preview ?? "")} />
          ) : (
            preview
          )}
        </span>
      )}
    </span>
  );
  const marks = (
    <>
      {item.flagged && (
        <Flag aria-hidden className="size-(--ui-icon-small) shrink-0 fill-current text-warning" strokeWidth={1.75} />
      )}
      {activity && <ActivityIcon activity={activity} />}
      <RowMarks item={item} />
    </>
  );
  // Stage 8: which account, in words, wherever the avatar ring's colour tells accounts apart.
  const accountElement = decoration?.account ? (
    <span
      title={decoration.account}
      className="max-w-(--ui-row-account-width) shrink-0 truncate text-12 text-secondary"
      data-testid="row-account"
    >
      {decoration.account}
    </span>
  ) : null;
  const timeElement = (
    <time
      dateTime={new Date(item.date).toISOString()}
      title={formatFullDate(item.date, i18n.language)}
      className={cx("shrink-0 text-12 tabular-nums", item.unread ? "text-primary" : "text-secondary")}
    >
      {time}
    </time>
  );

  return (
    <div
      className={cx(
        "relative flex h-full items-center gap-3 rounded-card px-3",
        selected ? "bg-selected" : "group-hover/row:bg-hover",
        // The list keeps focus (aria-activedescendant); the cursor row shows it.
        cursor && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
      )}
    >
      <SelectionMark unread={item.unread} selected={selected} multiSelect={multiSelect} />
      <Avatar name={person?.name} email={person?.email ?? ""} accountColor={accountColor} />
      {twoLine ? (
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex min-w-0 items-center gap-2">
            {sender}
            {marks}
            {accountElement}
            {timeElement}
          </span>
          {subjectLine}
        </span>
      ) : (
        <>
          {sender}
          {subjectLine}
          {marks}
          {accountElement}
          {timeElement}
        </>
      )}
      {actions && (
        // Covers the time column while shown, on the row's own background.
        <div
          aria-hidden
          className={cx(
            "absolute end-2 flex items-center gap-1 rounded-control ps-2",
            twoLine ? "top-1" : "inset-y-0",
            selected ? "bg-selected" : "bg-hover",
            "invisible group-hover/row:visible",
            cursor && "group-focus-visible/list:visible",
          )}
          data-testid="row-actions"
        >
          {actions}
        </div>
      )}
    </div>
  );
}
