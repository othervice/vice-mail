import { Flag, Paperclip } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { AccountColor, Address, MessageListItem } from "../../services/ipc";
import { Avatar } from "../accounts/Avatar";
import { formatFullDate, formatListDate, type DayBounds } from "./rows";

interface MessageRowContentProps {
  item: MessageListItem;
  accountColor: AccountColor;
  bounds: DayBounds;
  /** Sent and drafts show the recipients where other folders show the sender. */
  showRecipients: boolean;
  twoLine: boolean;
  selected: boolean;
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

/**
 * The row's accessible name, in reading order: state, who, subject, attachment, flag, full date.
 * Explicit, because a name computed from the row's parts would run words together.
 */
export function useRowLabel(item: MessageListItem, showRecipients: boolean): string {
  const { t, i18n } = useTranslation();
  const who = useWho(item, showRecipients);
  return [
    item.unread && t("list.unread"),
    who,
    item.subject.trim() || t("list.noSubject"),
    item.hasAttachments && t("list.attachment"),
    item.flagged && t("list.flagged"),
    formatFullDate(item.date, i18n.language),
  ]
    .filter(Boolean)
    .join(", ");
}

/**
 * One message in the list (docs/DESIGN.md, "Zeile"): unread dot, avatar, sender, subject with
 * attachment symbol, flag, time. The option around it carries the accessible name
 * (`useRowLabel`). No preview text and no quick actions yet (stages 3 and 4).
 */
export function MessageRowContent({ item, accountColor, bounds, showRecipients, twoLine, selected }: MessageRowContentProps) {
  const { t, i18n } = useTranslation();
  const person = showRecipients ? item.to[0] : item.from;
  const who = useWho(item, showRecipients);
  const subject = item.subject.trim() || t("list.noSubject");
  const time = formatListDate(item.date, bounds, i18n.language);

  const dot = (
    <span className="flex w-2 shrink-0 justify-center">
      {item.unread && <span aria-hidden className="size-(--ui-dot) rounded-full bg-accent" />}
    </span>
  );
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
      {who}
    </span>
  );
  const subjectLine = (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span title={subject} className={cx("truncate text-14", item.subject.trim() ? "text-primary" : "text-tertiary")}>
        {subject}
      </span>
      {item.hasAttachments && (
        <Paperclip aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
      )}
    </span>
  );
  const flag = item.flagged && (
    <Flag aria-hidden className="size-(--ui-icon-small) shrink-0 fill-current text-warning" strokeWidth={1.75} />
  );
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
        "flex h-full items-center gap-3 rounded-card px-3",
        selected ? "bg-selected" : "group-hover/row:bg-hover",
        // The list keeps focus (aria-activedescendant); the selected row shows it.
        selected && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
      )}
    >
      {dot}
      <Avatar name={person?.name} email={person?.email ?? ""} accountColor={accountColor} />
      {twoLine ? (
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex min-w-0 items-center gap-2">
            {sender}
            {flag}
            {timeElement}
          </span>
          {subjectLine}
        </span>
      ) : (
        <>
          {sender}
          {subjectLine}
          {flag}
          {timeElement}
        </>
      )}
    </div>
  );
}
