import { Check, ChevronRight, Clock, Flag, Paperclip, TriangleExclamationPoint } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { HighlightedText } from "../../components/HighlightedText";
import type { AccountColor, Address, MessageActivity, MessageListItem, SearchHighlights, TableColumnId } from "../../services/ipc";
import { useLayoutStore } from "../../stores/layout";
import { accountFill } from "../accounts/accountColor";
import { RowMarks, useRowMarkLabels, type RowMark } from "../comfort/RowMarks";
import { formatSize } from "../reading/format";
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
  /**
   * Stage 17 (decision 0049): in a conversation tree, whether the row's conversation is unfolded
   * (its chevron turns, its line runs down to the messages below); undefined: it cannot unfold.
   */
  expanded?: boolean | undefined;
  /**
   * Stage 21 (decision 0064): the context menu is open for this row, which is not part of the
   * selection: a dashed ring shows what the menu acts on (like Thunderbird), the selection stays.
   */
  contextTarget?: boolean | undefined;
}

/** Stage 21: the ring of a row the context menu acts on outside the selection. */
const CONTEXT_TARGET = "outline-2 -outline-offset-2 outline-dashed outline-focus-ring";

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
  /**
   * Stage 17 (decision 0049): the conversation's subject, shown instead of the message's (its
   * oldest message's without reply and forward prefixes, like Thunderbird).
   */
  subject?: string;
  /**
   * Stage 17: the conversation's thread (null while not threaded): the identity of the row in a
   * conversation tree, which stays while a newer message becomes the row's.
   */
  threadId?: number | null;
}

/** Where `trimmed` starts in `raw`, so ranges into `raw` still fit after trimming. */
function leadingSpace(raw: string): number {
  return raw.length - raw.trimStart().length;
}

/**
 * The subject a row shows (stage 17: a conversation's, else the message's), and where it starts
 * in the message's subject, so the search highlights of that subject still fit; null when it is
 * another text (the conversation's subject differs from the hit's).
 */
export function rowSubject(item: MessageListItem, decoration: RowDecoration | undefined): { text: string; offset: number | null } {
  const shown = decoration?.subject?.trim();
  if (shown === undefined) return { text: item.subject.trim(), offset: leadingSpace(item.subject) };
  const own = item.subject.trimEnd();
  return { text: shown, offset: shown && own.endsWith(shown) ? own.length - shown.length : null };
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
    rowSubject(item, decoration).text || t("list.noSubject"),
    decoration?.account && t("presentation.rowAccount", { name: decoration.account }),
    threadCount > 1 && t("threads.replies", { count: threadCount - 1 }),
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

/** The selection box itself: checked on selected rows (accent fill), else an empty square. */
function CheckBox({ selected }: { selected: boolean }) {
  return (
    <span
      className={cx(
        "flex size-(--ui-icon-small) items-center justify-center rounded-control border",
        selected ? "border-accent bg-accent-fill text-on-accent" : "border-secondary bg-surface-raised",
      )}
    >
      {selected && <Check className="size-3" strokeWidth={3} />}
    </span>
  );
}

/**
 * The row's mark column, narrow and fixed at its start, in every presentation (rows, child rows,
 * the table; stage 19, decision 0061, and since stage 22 without avatars, decision 0072): the
 * unread dot, which turns into the selection box on hover and while several messages are selected
 * (in every row then). Unread rows are bold as well, so unread never rests on the dot alone. A
 * mouse target only (`data-row-check`, handled by the list): the option's `aria-selected` says
 * the same to screen readers, and Space or X toggles it from the keyboard. Positioned, so it
 * covers an unfolded conversation's line that runs under it (stage 17, `TreeLine`).
 */
function SelectionMark({ unread, selected, multiSelect }: { unread: boolean; selected: boolean; multiSelect: boolean }) {
  return (
    <span aria-hidden data-row-check className="relative flex size-(--ui-row-mark) shrink-0 items-center justify-center" data-testid="row-mark">
      {unread && (
        <span
          className={cx("size-(--ui-dot) rounded-full bg-accent", multiSelect ? "hidden" : "group-hover/row:hidden")}
          data-testid="row-unread-dot"
        />
      )}
      <span className={cx("absolute inset-0 items-center justify-center", multiSelect ? "flex" : "hidden group-hover/row:flex")} data-testid="row-check">
        <CheckBox selected={selected} />
      </span>
    </span>
  );
}

/**
 * Stage 17 (decision 0049): the pill after the subject, "3 replies" with a chevron, like
 * Thunderbird's; a click unfolds or folds the conversation below the row. A mouse target like the
 * selection box (`data-row-toggle`, handled by the list): the tree item says "expanded" or
 * "collapsed" itself, and → and ← do the same from the keyboard. Unfolded, it takes the accent.
 *
 * The subject takes the width first: short of room, the pill gives up its word before the
 * subject loses a letter ("3 replies", "3 repl…", "3"), never its chevron and number. A grid whose
 * word column may shrink to nothing (`minmax(0, max-content)`) makes chevron and number the pill's
 * smallest size, and its shrink factor makes the row's line take from it first: a million times
 * the subject's, so the subject's share of a shortage stays far below a 64th of a pixel (WebKit's
 * layout unit) and no ellipsis appears while the pill can still give.
 */
function RepliesToggle({ replies, expanded }: { replies: number; expanded: boolean | undefined }) {
  const { t } = useTranslation();
  const foldable = expanded !== undefined;
  const word = ` ${t("threads.repliesWord", { count: replies })}`;
  return (
    <span
      aria-hidden
      {...(foldable && { "data-row-toggle": true })}
      title={foldable ? t(expanded ? "threads.collapse" : "threads.expand") : t("threads.replies", { count: replies })}
      // Stage 23 (decision 0084): a small framed count, as in the draft.
      className={cx(
        "grid h-4 shrink-1000000 grid-cols-[auto_auto_minmax(0,max-content)] items-center rounded-chip border text-11 font-semibold tabular-nums transition-colors",
        "px-1",
        foldable && "cursor-pointer",
        expanded ? "border-accent text-accent" : "border-border-subtle text-secondary",
        foldable && !expanded && "hover:text-primary",
      )}
      data-testid="row-replies"
      data-expanded={foldable ? expanded : undefined}
    >
      {foldable ? (
        <ChevronRight className={cx("size-3", expanded && "rotate-90")} strokeWidth={2.25} />
      ) : (
        <span />
      )}
      <span className="whitespace-nowrap">{replies}</span>
      {/* The space belongs to the word, so it goes with it. Stage 21 (decision 0071): the column is
          as wide as the word (an invisible copy of it, `::before`) and shrinks like before; the
          word lies over it and hides once less than 2.5 em of it would show, so a sliver of the
          ellipsis ("3 .") never does. */}
      <span
        data-word={word}
        className="relative min-w-0 self-stretch overflow-hidden whitespace-pre before:invisible before:block before:h-0 before:content-[attr(data-word)]"
        data-testid="row-replies-word"
      >
        <span className="absolute inset-0 @container">
          <span className="block overflow-hidden leading-4 text-ellipsis whitespace-pre @max-[2.5em]:invisible">{word}</span>
        </span>
      </span>
    </span>
  );
}

/**
 * Stage 18 (decision 0052): the quick actions of a row, while the pointer is over it or the
 * keyboard cursor is on it. They take the place of the row's meta (marks, account, time) at the
 * end of its line, as a flex item of that line, not as a layer over it: the line's text gives way
 * to them (the preview first, then the pill's word, then the subject; in narrow rows the sender
 * on the first line), and nothing a click can reach (the selection box at the start, the
 * replies toggle after the subject) is ever under them. Rather than a layer that needs room it
 * cannot know, the row lays itself out again with the bar in it.
 *
 * 24 px targets (WCAG 2.5.8) in a bar as high as the smallest row allows (compact child rows,
 * 28 px). In narrow rows the bar sits on the first line (sender and time) and reaches 4 px above
 * it (`lifted`), never down into the subject's line with the toggle.
 */
function RowActions({ cursor, lifted, children }: { cursor: boolean; lifted?: boolean; children: ReactNode }) {
  return (
    <span
      aria-hidden
      className={cx(
        "hidden shrink-0 items-center rounded-tile bg-surface-list px-1 shadow-floating group-hover/row:flex",
        cursor && "group-focus-visible/list:flex",
        lifted && "self-end",
      )}
      data-testid="row-actions"
      data-row-actions
    >
      {children}
    </span>
  );
}

/**
 * The row's meta (marks, time): in the line as before, gone while the quick actions show. Stage 22
 * (decision 0072): the account's label between them stays, before the actions.
 */
function RowMeta({ actions, cursor, children, ...rest }: { actions: boolean; cursor: boolean; children: ReactNode; "data-testid"?: string }) {
  return (
    <span
      className={cx("contents", actions && "group-hover/row:hidden", actions && cursor && "group-focus-visible/list:hidden")}
      data-testid={rest["data-testid"] ?? "row-meta"}
    >
      {children}
    </span>
  );
}

/**
 * Stage 17: the dotted line of an unfolded conversation, under the mark column of its row (stage
 * 22: where the avatar was, decision 0072): from the mark down in the conversation's row, through
 * every child row, ending at the last one's mark. Decoration only. In a one-line row it starts at
 * the row's middle, where the mark is (`mark`); in a narrow row at the middle of the first line,
 * where the mark is there (`firstLine`, inside a box that starts at the content's top, see
 * `MessageRowContent`). The unread dot, positioned after it, covers its start.
 */
function TreeLine({ from, to }: { from: "top" | "mark" | "firstLine"; to: "bottom" | "middle" }) {
  return (
    <span
      aria-hidden
      className={cx(
        "pointer-events-none absolute border-s border-dotted border-border-strong",
        from === "firstLine" ? "start-[calc(var(--ui-row-mark)/2)]" : "start-(--ui-tree-line-start)",
        from === "top" ? "top-0" : from === "mark" ? "top-1/2" : "top-[calc(var(--ui-space-5)/2)]",
        to === "bottom" ? "bottom-0" : "h-1/2",
      )}
      data-testid="tree-line"
    />
  );
}

/**
 * One message in the list (docs/DESIGN.md, "Zeile"): the mark column (unread dot, the selection
 * box on hover), sender, subject with conversation size and attachment symbol, preview, flag,
 * pending or failed change, the account's dot and name in lists that mix accounts, time,
 * and the quick actions on hover. No avatar (stage 22, decision 0072). Wide lists show one line;
 * narrow ones three (sender and time, subject, preview), compact ones two (decision 0031). The
 * option around it carries the accessible name (`useRowLabel`); the preview describes it.
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
  expanded,
  contextTarget = false,
}: MessageRowContentProps) {
  const { t, i18n } = useTranslation();
  const compact = useLayoutStore((state) => state.density === "compact");
  const who = useWho(item, showRecipients, decoration);
  // Search highlights mark the message's own sender, not a list of a conversation's senders.
  const ownSender = (decoration?.participants?.length ?? 0) <= 1;
  const shownSubject = rowSubject(item, decoration);
  const subject = shownSubject.text || t("list.noSubject");
  const time = formatListDate(item.date, bounds, i18n.language);
  const activity = rowActivity(item);
  const highlights = decoration?.highlights;
  const preview = item.preview?.trim();
  const threadCount = decoration?.threadCount ?? 1;
  // Narrow comfortable rows give the preview a line of its own.
  const threeLine = twoLine && !compact;

  const senderText = (
    <span
      title={who}
      className={cx("min-w-0 truncate text-13 text-primary", item.unread ? "font-bold" : "font-semibold")}
      data-testid="row-sender"
    >
      {highlights && !showRecipients && item.from && ownSender ? (
        <HighlightedText text={who} ranges={highlights.from} offset={item.from.name?.trim() ? leadingSpace(item.from.name) : 0} />
      ) : (
        who
      )}
    </span>
  );
  // The sender's column (narrow: as wide as the names; the account's name beside them gives way
  // first).
  const sender = <span className={cx("flex min-w-0 items-center", !twoLine && "w-(--ui-list-sender-width) shrink-0")}>{senderText}</span>;
  const previewElement = preview && (
    <span
      className={cx("min-w-0 truncate text-12 text-tertiary", !threeLine && "flex-1 basis-0")}
      data-testid="row-preview"
    >
      {highlights ? (
        <HighlightedText text={preview} ranges={highlights.preview} offset={leadingSpace(item.preview ?? "")} />
      ) : (
        preview
      )}
    </span>
  );
  const flag = item.flagged && (
    <Flag aria-hidden className="size-(--ui-icon-small) shrink-0 fill-current text-warning" strokeWidth={1.75} data-testid="row-flag" />
  );
  // The subject comes first; in one line the preview takes what is left (basis 0), down to nothing.
  // Stage 23 (decision 0084): in narrow rows the subject's line ends with the count, the
  // attachment, the flag and the marks, as in the draft.
  const subjectLine = (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span
        title={subject}
        className={cx(
          "min-w-0 truncate text-13",
          !shownSubject.text ? "text-tertiary" : item.unread ? "font-semibold text-primary" : "text-primary",
        )}
        data-testid="row-subject"
      >
        {highlights && shownSubject.text && shownSubject.offset !== null ? (
          <HighlightedText text={subject} ranges={highlights.subject} offset={shownSubject.offset} />
        ) : (
          subject
        )}
      </span>
      {/* Narrow rows: count and symbols at the line's end. A spacer, not a growing subject, so the
          pill's word still gives way before the subject loses a letter (stage 17). */}
      {twoLine && <span aria-hidden className="min-w-0 flex-1" />}
      {threadCount > 1 && <RepliesToggle replies={threadCount - 1} expanded={expanded} />}
      {item.hasAttachments && (
        <Paperclip aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      )}
      {twoLine && (
        <span className="contents" data-testid="row-meta-marks">
          {flag}
          {activity && <ActivityIcon activity={activity} />}
          <RowMarks item={item} omit={omitMarks} />
        </span>
      )}
      {!threeLine && previewElement}
    </span>
  );
  const marks = (
    <>
      {flag}
      {activity && <ActivityIcon activity={activity} />}
      <RowMarks item={item} omit={omitMarks} />
    </>
  );
  // Stage 23, step 3 (decision 0084): in lists that mix accounts a 3 px stripe in the account's
  // colour at the row's right edge (it was its dot and name before the time, stage 8 and 17). The
  // name stays in the row's accessible name and is the stripe's tooltip; the open message names
  // its account above the subject, the table has its column.
  const stripe = decoration?.account ? (
    <span
      aria-hidden
      title={decoration.account}
      className={cx("absolute inset-y-2 end-(--ui-row-stripe-inset) w-(--ui-row-stripe) rounded-full", accountFill(accountColor))}
      data-testid="row-account"
    />
  ) : null;
  const timeElement = (
    <time
      dateTime={new Date(item.date).toISOString()}
      title={formatFullDate(item.date, i18n.language)}
      className="shrink-0 text-11 text-tertiary tabular-nums"
    >
      {time}
    </time>
  );
  const mark = <SelectionMark unread={item.unread} selected={selected} multiSelect={multiSelect} />;
  const rowClass = cx(
    // Stage 23 (decision 0084): a hairline under every row; room at the end for the stripe.
    "relative h-full rounded-row border-b border-border-subtle ps-(--ui-row-lead) transition-colors",
    stripe ? "pe-5" : "pe-4",
    selected ? "bg-selected-muted" : "group-hover/row:bg-hover",
    // The list keeps focus (aria-activedescendant); the cursor row shows it.
    cursor && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
    contextTarget && CONTEXT_TARGET,
  );

  // Hidden from screen readers: the option is named by its visually hidden text (useRowLabel).
  if (twoLine) {
    // Narrow: the content in the middle of a grid (rows 1fr, auto, 1fr), the mark on its first
    // line. Stage 17: the unfolded conversation's line leaves from the mark, in a box from the
    // content's top to the row's bottom, so it needs no heights.
    return (
      <div
        aria-hidden
        className={cx(rowClass, "grid grid-rows-[1fr_auto_1fr]")}
        data-row-selected={selected || undefined}
        data-context-target={contextTarget || undefined}
      >
        {expanded && (
          <span aria-hidden className="pointer-events-none relative col-start-1 row-start-2 row-end-4">
            <TreeLine from="firstLine" to="bottom" />
          </span>
        )}
        <span className="col-start-1 row-start-2 flex min-w-0 items-start gap-1">
          <span className="flex h-5 shrink-0 items-center">{mark}</span>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="flex h-5 min-w-0 items-center gap-2">
              {sender}
              <span className="flex-1" />
              <RowMeta actions={Boolean(actions)} cursor={cursor}>
                {timeElement}
              </RowMeta>
              {/* Stage 18: on the first line, clear of the subject's line with the toggle. */}
              {actions && (
                <RowActions cursor={cursor} lifted>
                  {actions}
                </RowActions>
              )}
            </span>
            {subjectLine}
            {threeLine && previewElement}
          </span>
        </span>
        {stripe}
      </div>
    );
  }
  return (
    <div
      aria-hidden
      className={cx(rowClass, "flex items-center")}
      data-row-selected={selected || undefined}
      data-context-target={contextTarget || undefined}
    >
      {/* Stage 17: the unfolded conversation's line leaves from under the mark. */}
      {expanded && <TreeLine from="mark" to="bottom" />}
      <span className="flex min-w-0 flex-1 items-center gap-2">
        {mark}
        <span className="flex min-w-0 flex-1 items-center gap-3">
          {sender}
          {subjectLine}
          <RowMeta actions={Boolean(actions)} cursor={cursor} data-testid="row-meta-marks">
            {marks}
          </RowMeta>
          <RowMeta actions={Boolean(actions)} cursor={cursor}>
            {timeElement}
          </RowMeta>
          {actions && <RowActions cursor={cursor}>{actions}</RowActions>}
        </span>
      </span>
      {stripe}
    </div>
  );
}

interface TableRowContentProps extends Omit<MessageRowContentProps, "twoLine"> {
  /** The drawn columns, in order; their widths are the list's `--table-columns`. */
  columns: readonly TableColumnId[];
}

/**
 * Stage 19 (decision 0061): a message row of the table, the wide list's other presentation. The
 * same option around it (named by `useRowLabel`, hidden content here), the same selection box,
 * selection, focus ring, replies toggle and quick actions; the content in the table's columns
 * (CSS grid on `--table-columns`, which the header uses too). The quick actions take the end of
 * the subject's cell, so no column moves while they show.
 */
export function TableRowContent({
  item,
  columns,
  accountColor,
  bounds,
  showRecipients,
  selected,
  cursor,
  multiSelect,
  actions,
  decoration,
  omitMarks,
  expanded,
  contextTarget = false,
}: TableRowContentProps) {
  const { t, i18n } = useTranslation();
  const who = useWho(item, showRecipients, decoration);
  const shownSubject = rowSubject(item, decoration);
  const subject = shownSubject.text || t("list.noSubject");
  const activity = rowActivity(item);
  const threadCount = decoration?.threadCount ?? 1;
  const recipients = item.to.length > 0 ? item.to.map(addressName).join(", ") : "";
  const cell = (id: TableColumnId) => {
    switch (id) {
      case "flag":
        return (
          <span key={id} className="flex justify-center" data-testid="cell-flag">
            {item.flagged && <Flag className="size-(--ui-icon-small) fill-current text-warning" strokeWidth={1.75} />}
          </span>
        );
      case "attachment":
        return (
          <span key={id} className="flex justify-center" data-testid="cell-attachment">
            {item.hasAttachments && <Paperclip className="size-(--ui-icon-small) text-tertiary" strokeWidth={1.75} />}
          </span>
        );
      case "correspondent":
        return (
          <span
            key={id}
            title={who}
            className={cx("min-w-0 truncate pe-3 text-13 text-primary", item.unread ? "font-bold" : "font-semibold")}
            data-testid="cell-correspondent"
          >
            {who}
          </span>
        );
      case "recipient":
        return (
          <span key={id} title={recipients} className="min-w-0 truncate pe-3 text-13 text-secondary" data-testid="cell-recipient">
            {recipients}
          </span>
        );
      case "subject":
        return (
          <span key={id} className="flex min-w-0 items-center gap-2 pe-3" data-testid="cell-subject">
            <span
              title={subject}
              className={cx(
                "min-w-0 truncate text-13",
                !shownSubject.text ? "text-tertiary" : item.unread ? "font-semibold text-primary" : "text-primary",
              )}
            >
              {subject}
            </span>
            {threadCount > 1 && <RepliesToggle replies={threadCount - 1} expanded={expanded} />}
            <span className="flex-1" />
            <RowMeta actions={Boolean(actions)} cursor={cursor}>
              {activity && <ActivityIcon activity={activity} />}
              <RowMarks item={item} omit={omitMarks} />
            </RowMeta>
            {actions && <RowActions cursor={cursor}>{actions}</RowActions>}
          </span>
        );
      case "account":
        return (
          <span key={id} title={decoration?.account} className="flex min-w-0 items-center gap-1 pe-3" data-testid="cell-account">
            {decoration?.account && (
              <>
                <span className={cx("size-(--ui-dot) shrink-0 rounded-mark", accountFill(accountColor))} />
                <span className="min-w-0 truncate text-12 text-secondary">{decoration.account}</span>
              </>
            )}
          </span>
        );
      case "date":
        return (
          <time
            key={id}
            dateTime={new Date(item.date).toISOString()}
            title={formatFullDate(item.date, i18n.language)}
            className="min-w-0 truncate pe-3 text-11 text-tertiary tabular-nums"
            data-testid="cell-date"
          >
            {formatListDate(item.date, bounds, i18n.language)}
          </time>
        );
      case "size":
        return (
          <span key={id} className="min-w-0 truncate pe-3 text-end text-11 text-secondary tabular-nums" data-testid="cell-size">
            {item.size !== undefined ? formatSize(item.size, i18n.language) : ""}
          </span>
        );
    }
  };
  // Hidden from screen readers: the option is named by its visually hidden text (useRowLabel).
  return (
    <div
      aria-hidden
      className={cx(
        "relative flex h-full items-center rounded-row border-b border-border-subtle ps-(--ui-row-lead) pe-3 transition-colors",
        selected ? "bg-selected-muted" : "group-hover/row:bg-hover",
        cursor && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
        contextTarget && CONTEXT_TARGET,
      )}
      data-row-selected={selected || undefined}
      data-context-target={contextTarget || undefined}
      data-testid="table-row"
    >
      <span className="flex min-w-0 flex-1 items-center gap-3">
        <SelectionMark unread={item.unread} selected={selected} multiSelect={multiSelect} />
        <span className="grid min-w-0 flex-1 grid-cols-(--table-columns) items-center">{columns.map(cell)}</span>
      </span>
    </div>
  );
}

interface ChildRowContentProps {
  item: MessageListItem;
  /** The folder's name where the message lies outside the conversation row's folder. */
  folder: string | null;
  bounds: DayBounds;
  selected: boolean;
  cursor: boolean;
  multiSelect: boolean;
  /** The last message of the conversation: its line ends here. */
  last: boolean;
  actions?: ReactNode;
  /** Stage 21: the context menu acts on this row outside the selection. */
  contextTarget?: boolean | undefined;
}

/**
 * Stage 17 (decision 0049): the accessible name of a child row, in reading order like a message
 * row's: state, sender, folder (when another), the start of the text, symbols in words, time.
 */
export function useChildLabel(item: MessageListItem, folder: string | null, bounds: DayBounds): string {
  const { t, i18n } = useTranslation();
  const activity = rowActivity(item);
  return [
    item.unread && t("list.unread"),
    item.from ? addressName(item.from) : t("list.unknownSender"),
    folder && t("threads.inFolder", { folder }),
    childText(item, t("list.noSubject")),
    item.hasAttachments && t("list.attachment"),
    item.flagged && t("list.flagged"),
    activity && t(`list.activity.${activity}`),
    formatListDate(item.date, bounds, i18n.language),
  ]
    .filter(Boolean)
    .join(", ");
}

/** What a child row says after the sender: the start of the text, else the subject. */
function childText(item: MessageListItem, noSubject: string): string {
  return item.preview?.trim() || item.subject.trim() || noSubject;
}

/**
 * Stage 17 (decision 0049): one message of an unfolded conversation, a compact single line in
 * every layout: the mark column on the conversation's dotted line (unread dot, the selection box
 * on hover; stage 22 without the sender's mini avatar, decision 0072), the sender, the folder
 * where it lies elsewhere (the user's own answer in Sent), the start of the text, symbols, and
 * the time. Same tokens, selection, focus and quick actions as a message row.
 */
export function ChildRowContent({ item, folder, bounds, selected, cursor, multiSelect, last, actions, contextTarget = false }: ChildRowContentProps) {
  const { t, i18n } = useTranslation();
  const sender = item.from ? addressName(item.from) : t("list.unknownSender");
  const text = childText(item, t("list.noSubject"));
  const activity = rowActivity(item);
  return (
    <div
      aria-hidden
      className={cx(
        "relative flex h-full items-center rounded-row ps-(--ui-row-lead) pe-3 transition-colors",
        last && "border-b border-border-subtle",
        selected ? "bg-selected-muted" : "group-hover/row:bg-hover",
        cursor && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
        contextTarget && CONTEXT_TARGET,
      )}
      data-row-selected={selected || undefined}
      data-context-target={contextTarget || undefined}
    >
      <TreeLine from="top" to={last ? "middle" : "bottom"} />
      <span className="flex min-w-0 flex-1 items-center gap-2">
        {/* The mark column of the conversation's row, on its line. */}
        <SelectionMark unread={item.unread} selected={selected} multiSelect={multiSelect} />
        <span className="flex min-w-0 flex-1 items-center gap-3">
        {/* As wide as they are: the sender's name (at most a third), the folder; the text the rest.
            Clipped: with the quick actions in a narrow list the folder may not fit beside them. */}
        <span className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
          <span
            title={item.from?.email ?? sender}
            className={cx("max-w-1/3 shrink-0 truncate text-13 text-primary", item.unread ? "font-semibold" : "font-regular")}
            data-testid="child-sender"
          >
            {sender}
          </span>
          {folder && (
            <span title={folder} className="max-w-(--ui-row-account-width) shrink-0 truncate rounded-full bg-badge px-2 text-12 text-secondary" data-testid="child-folder">
              {folder}
            </span>
          )}
          <span title={text} className={cx("min-w-0 flex-1 truncate text-12", item.unread ? "text-secondary" : "text-tertiary")} data-testid="child-text">
            {text}
          </span>
        </span>
        <RowMeta actions={Boolean(actions)} cursor={cursor}>
          {item.hasAttachments && <Paperclip aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />}
          {item.flagged && <Flag aria-hidden className="size-(--ui-icon-small) shrink-0 fill-current text-warning" strokeWidth={1.75} />}
          {activity && <ActivityIcon activity={activity} />}
          <time
            dateTime={new Date(item.date).toISOString()}
            title={formatFullDate(item.date, i18n.language)}
            className="shrink-0 text-11 text-tertiary tabular-nums"
          >
            {formatListDate(item.date, bounds, i18n.language)}
          </time>
        </RowMeta>
        {actions && <RowActions cursor={cursor}>{actions}</RowActions>}
        </span>
      </span>
    </div>
  );
}

/**
 * Stage 17: a row of an unfolded conversation that is no message: its messages are loading, could
 * not be loaded (a tree item: Enter, Space or a click try again), or are cut to the newest.
 */
export function ChildStatusRow({ kind, text, last }: { kind: "loading" | "error" | "truncated"; text: string; last: boolean }) {
  return (
    <div
      className={cx(
        "relative flex h-full items-center gap-2 rounded-row ps-(--ui-row-lead) pe-3 text-12 transition-colors",
        kind === "error" ? "text-danger group-hover/row:bg-hover" : "text-tertiary",
      )}
      data-testid={`child-status-${kind}`}
    >
      <TreeLine from="top" to={last ? "middle" : "bottom"} />
      <span aria-hidden className="w-(--ui-row-mark) shrink-0" />
      {kind === "loading" ? (
        <>
          <span className="sr-only">{text}</span>
          <span aria-hidden className="h-3 w-(--ui-list-sender-width) max-w-1/3 rounded-full bg-badge" />
          <span aria-hidden className="h-3 flex-1 rounded-full bg-hover" />
        </>
      ) : (
        <span className={cx("min-w-0 truncate", kind === "error" && "font-medium group-hover/row:underline")}>{text}</span>
      )}
    </div>
  );
}
