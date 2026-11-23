import { Check, Clock, Flag, Paperclip, TriangleExclamationPoint } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { HighlightedText } from "../../components/HighlightedText";
import type { AccountColor, Address, MessageActivity, MessageListItem, SearchHighlights } from "../../services/ipc";
import { useLayoutStore } from "../../stores/layout";
import { Avatar } from "../accounts/Avatar";
import { RowMarks, useRowMarkLabels, type RowMark } from "../comfort/RowMarks";
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
  /** Quick actions, shown on hover and on the keyboard cursor (mouse shortcuts, see MessageList). */
  actions?: ReactNode;
  /** Stage 7: conversation size and search highlights. */
  decoration?: RowDecoration | undefined;
  /** Decision 0031: marks the surroundings already show (the priority card's "important"). */
  omitMarks?: readonly RowMark[] | undefined;
}

/** Stage 7: what a row of a conversation list or of search results adds to the message. */
export interface RowDecoration {
  /** Messages in the conversation; shown after the subject when more than one. */
  threadCount?: number;
  /**
   * Decision 0039: the senders of the conversation, oldest first; with more than one the row names
   * them all instead of the sender of its message.
   */
  participants?: readonly Address[];
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

/** Who a row names: the sender, or the recipients in Sent and Drafts; a conversation's senders. */
function useWho(item: MessageListItem, showRecipients: boolean, decoration?: RowDecoration): string {
  const { t } = useTranslation();
  if (showRecipients) {
    return item.to.length > 0 ? t("list.to", { names: item.to.map(addressName).join(", ") }) : t("list.unknownSender");
  }
  const participants = decoration?.participants ?? [];
  if (participants.length > 1) return participants.map(addressName).join(", ");
  return item.from ? addressName(item.from) : t("list.unknownSender");
}

/** The row's activity, absent for most rows (the core leaves the field out). */
export function rowActivity(item: MessageListItem): MessageActivity | null {
  return item.activity ?? null;
}

/**
 * The text that names a row, in reading order: state, who, subject, account (lists that mix
 * accounts), conversation size, attachment, flag, pending or failed change, marks, preview, time as
 * shown. The option renders it as visually hidden content (decision 0031, WCAG 2.5.3): every
 * visible word of the row is in it, the symbols are said in words, and the parts do not run
 * together as a name computed from the drawn row would.
 */
export function useRowLabel(
  item: MessageListItem,
  showRecipients: boolean,
  decoration: RowDecoration | undefined,
  bounds: DayBounds,
): string {
  const { t, i18n } = useTranslation();
  const who = useWho(item, showRecipients, decoration);
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
    item.preview?.trim(),
    formatListDate(item.date, bounds, i18n.language),
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
          selected ? "border-accent bg-accent-fill text-on-accent" : "border-secondary bg-surface-raised",
          boxShown || multiSelect ? "flex" : "hidden group-hover/row:flex",
        )}
        data-testid="row-check"
      >
        {selected && <Check className="size-3" strokeWidth={3} />}
      </span>
    </span>
  );
}

/** Small pill after the subject: messages in the conversation (decision 0031). */
function ThreadCount({ count }: { count: number }) {
  const { i18n } = useTranslation();
  return (
    <span
      aria-hidden
      className="shrink-0 rounded-full bg-badge px-2 text-12 font-medium text-secondary tabular-nums"
      data-testid="row-thread-count"
    >
      {new Intl.NumberFormat(i18n.language).format(count)}
    </span>
  );
}

/**
 * One message in the list (docs/DESIGN.md, "Zeile"): unread dot (selection box on hover), avatar,
 * sender, subject with conversation size and attachment symbol, preview, flag, pending or failed
 * change, time, and the quick actions on hover. Wide lists show one line; narrow ones three
 * (sender and time, subject, preview), compact ones two (decision 0031). The option around it
 * carries the accessible name (`useRowLabel`); the preview describes it.
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
  actions,
  decoration,
  omitMarks,
}: MessageRowContentProps) {
  const { t, i18n } = useTranslation();
  const compact = useLayoutStore((state) => state.density === "compact");
  const person = showRecipients ? item.to[0] : item.from;
  const who = useWho(item, showRecipients, decoration);
  // Search highlights mark the message's own sender, not a list of a conversation's senders.
  const ownSender = (decoration?.participants?.length ?? 0) <= 1;
  const subject = item.subject.trim() || t("list.noSubject");
  const time = formatListDate(item.date, bounds, i18n.language);
  const activity = rowActivity(item);
  const highlights = decoration?.highlights;
  const preview = item.preview?.trim();
  const threadCount = decoration?.threadCount ?? 1;
  // Narrow comfortable rows give the preview a line of its own.
  const threeLine = twoLine && !compact;

  const sender = (
    <span
      title={who}
      className={cx(
        "truncate text-14 text-primary",
        item.unread ? "font-semibold" : "font-medium",
        !twoLine && "w-(--ui-list-sender-width) shrink-0",
        twoLine && "min-w-0 flex-1",
      )}
    >
      {highlights && !showRecipients && item.from && ownSender ? (
        <HighlightedText text={who} ranges={highlights.from} offset={item.from.name?.trim() ? leadingSpace(item.from.name) : 0} />
      ) : (
        who
      )}
    </span>
  );
  const previewElement = preview && (
    <span
      className={cx("min-w-0 truncate text-13 text-tertiary", !threeLine && "flex-1 basis-0")}
      data-testid="row-preview"
    >
      {highlights ? (
        <HighlightedText text={preview} ranges={highlights.preview} offset={leadingSpace(item.preview ?? "")} />
      ) : (
        preview
      )}
    </span>
  );
  // The subject comes first; in one line the preview takes what is left (basis 0), down to nothing.
  const subjectLine = (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span
        title={subject}
        className={cx(
          "min-w-0 truncate",
          twoLine ? "text-13" : "text-14",
          !item.subject.trim() ? "text-tertiary" : item.unread ? "font-medium text-primary" : "text-primary",
        )}
      >
        {highlights && item.subject.trim() ? (
          <HighlightedText text={subject} ranges={highlights.subject} offset={leadingSpace(item.subject)} />
        ) : (
          subject
        )}
      </span>
      {threadCount > 1 && <ThreadCount count={threadCount} />}
      {item.hasAttachments && (
        <Paperclip aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      )}
      {!threeLine && previewElement}
    </span>
  );
  const marks = (
    <>
      {item.flagged && (
        <Flag aria-hidden className="size-(--ui-icon-small) shrink-0 fill-current text-warning" strokeWidth={1.75} />
      )}
      {activity && <ActivityIcon activity={activity} />}
      <RowMarks item={item} omit={omitMarks} />
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
      className={cx("shrink-0 text-12 tabular-nums", item.unread ? "font-medium text-primary" : "text-secondary")}
    >
      {time}
    </time>
  );
  // The account ring only where rows of several accounts mix (decision 0031).
  const avatar = (
    <Avatar name={person?.name} email={person?.email ?? ""} accountColor={accountColor} ring={decoration?.account !== undefined} />
  );

  // Hidden from screen readers: the option is named by its visually hidden text (useRowLabel).
  return (
    <div
      aria-hidden
      className={cx(
        "relative flex h-full items-center rounded-row px-3 transition-colors",
        selected ? "bg-selected" : "group-hover/row:bg-hover",
        // The list keeps focus (aria-activedescendant); the cursor row shows it.
        cursor && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
      )}
    >
      {twoLine ? (
        // Narrow: the dot sits on the first line, the avatar's top on the sender's.
        <span className="flex min-w-0 flex-1 items-start gap-3">
          <span className="flex h-5 shrink-0 items-center">
            <SelectionMark unread={item.unread} selected={selected} multiSelect={multiSelect} />
          </span>
          {avatar}
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="flex h-5 min-w-0 items-center gap-2">
              {sender}
              {marks}
              {accountElement}
              {timeElement}
            </span>
            {subjectLine}
            {threeLine && previewElement}
          </span>
        </span>
      ) : (
        <span className="flex min-w-0 flex-1 items-center gap-3">
          <SelectionMark unread={item.unread} selected={selected} multiSelect={multiSelect} />
          {avatar}
          {sender}
          {subjectLine}
          {marks}
          {accountElement}
          {timeElement}
        </span>
      )}
      {actions && (
        // A small floating bar at the row's end (decision 0031), over the time column and, in
        // narrow rows, the ends of the lines; it never covers the sender at the start.
        <div
          aria-hidden
          className={cx(
            "absolute inset-y-0 end-2 flex items-center",
            "invisible group-hover/row:visible",
            cursor && "group-focus-visible/list:visible",
          )}
          data-testid="row-actions"
        >
          <div className="flex items-center rounded-row bg-surface-list p-1 shadow-floating">{actions}</div>
        </div>
      )}
    </div>
  );
}
