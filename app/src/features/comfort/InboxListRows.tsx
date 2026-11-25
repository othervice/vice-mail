import { AlarmClockCheck, Bell, ChevronDown, ChevronRight, MailWarning, Newspaper, Pin, Zap, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { Address, MessageCategory } from "../../services/ipc";
import { useLayoutStore } from "../../stores/layout";
import { Avatar } from "../accounts/Avatar";
import { useNarrowList, useTreeList } from "../messages/listLayout";
import type { InboxBlock, InboxBundle } from "./inboxRows";

const BLOCK_ICONS: Record<InboxBlock, LucideIcon> = {
  pinned: Pin,
  returned: AlarmClockCheck,
  followUps: MailWarning,
  priority: Zap,
};

export const BUNDLE_ICONS: Record<Exclude<MessageCategory, "person">, LucideIcon> = {
  notification: Bell,
  newsletter: Newspaper,
};

function senderName(address: Address): string {
  return address.name?.trim() || address.email;
}

/** Heading of a block above the dated rows; "Priority" opens its card (docs/DESIGN.md). */
export function BlockHeading({ block }: { block: InboxBlock }) {
  const { t } = useTranslation();
  const Icon = BLOCK_ICONS[block];
  const priority = block === "priority";
  return (
    <div
      aria-hidden
      data-testid={`inbox-block-${block}`}
      className={cx(
        "flex h-full gap-2 px-3 font-semibold",
        priority ? "items-center text-13 text-primary" : "items-end pb-1 text-12 text-tertiary",
      )}
    >
      <Icon
        className={cx("size-(--ui-icon-small) shrink-0", priority && "text-warning")}
        strokeWidth={priority ? 2 : 1.75}
      />
      <span className="truncate">{t(`comfort.blocks.${block}`)}</span>
    </div>
  );
}

/**
 * "Show all (n)" closing the priority card: the important senders' messages as their own list.
 * Drawn only: the row around it is an option of the list that runs it (Enter, Space, click).
 */
export function ShowAllRow({ total, unread }: { total: number; unread: number }) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full items-center justify-center px-3" data-testid="priority-show-all">
      <span className="rounded-full px-3 py-1 text-12 font-medium text-accent group-hover/row:underline">
        {unread > 0 ? t("comfort.priority.showAllUnread", { count: total, unread }) : t("comfort.priority.showAll", { count: total })}
      </span>
    </div>
  );
}

interface BundleRowProps {
  bundle: InboxBundle;
  expanded: boolean;
}

/** Icon tiles of the bundles: a hue of their own each, from the avatar tints (decision 0031). */
const BUNDLE_TILES: Record<Exclude<MessageCategory, "person">, string> = {
  notification: "bg-avatar-1 text-avatar-1-text",
  newsletter: "bg-avatar-5 text-avatar-5-text",
};

/**
 * Notifications or newsletters of one date section as one row: icon tile, name, counter and chips
 * of the most frequent senders with mini avatars; Enter, Space or a click on the option around it
 * unfold the members in place below it. In a narrow list the chips go on the lines below the name
 * (decision 0031). Like a message row it is named by one visually hidden text that holds what
 * it shows, in reading order, plus the folded state: "Notifications, 6 · 2 new, Calendar 2, Bank,
 * +3, collapsed" (WCAG 2.5.3); the drawn row is hidden from screen readers.
 */
export function BundleRow({ bundle, expanded }: BundleRowProps) {
  const { t, i18n } = useTranslation();
  const narrow = useNarrowList();
  const tree = useTreeList();
  // Compact narrow rows have two lines: the chips keep to one.
  const compact = useLayoutStore((state) => state.density === "compact");
  const wrap = narrow && !compact;
  const category = bundle.category === "person" ? "notification" : bundle.category;
  const Icon = BUNDLE_ICONS[category];
  const name = t(`comfort.bundle.${category}`);
  const { count, unread, senders } = bundle.summary;
  const others = count - senders.reduce((sum, chip) => sum + chip.count, 0);
  const numbers = new Intl.NumberFormat(i18n.language);
  const Chevron = expanded ? ChevronDown : ChevronRight;
  const counter = unread > 0 ? t("comfort.bundle.counter", { count, unread }) : numbers.format(count);
  const more = others > 0 ? t("comfort.bundle.more", { count: others }) : null;
  const label = [
    name,
    counter,
    ...senders.map((chip) => (chip.count > 1 ? `${senderName(chip.sender)} ${numbers.format(chip.count)}` : senderName(chip.sender))),
    more,
    // A tree item says it itself (`aria-expanded`, stage 17).
    !tree && t(expanded ? "comfort.bundle.expanded" : "comfort.bundle.collapsed"),
  ]
    .filter(Boolean)
    .join(", ");
  const title = (
    <>
      <span className={cx("shrink-0 truncate text-14 text-primary", unread > 0 ? "font-semibold" : "font-medium")}>{name}</span>
      <span className="shrink-0 text-12 text-tertiary tabular-nums" data-testid="bundle-count">
        {counter}
      </span>
    </>
  );
  const chips = (
    <span className={cx("flex min-w-0 items-center gap-x-3 overflow-hidden", wrap && "flex-wrap gap-y-1", !narrow && "flex-1")}>
      {senders.map((chip) => (
        <span
          key={chip.sender.email}
          title={chip.sender.email}
          data-testid="bundle-chip"
          className={cx(
            "flex min-w-0 shrink items-center gap-1 text-12",
            chip.unread > 0 ? "font-medium text-primary" : "text-secondary",
          )}
        >
          <Avatar name={chip.sender.name} email={chip.sender.email} accountColor={0} ring={false} size="mini" />
          <span className="truncate" data-testid="bundle-chip-name">
            {senderName(chip.sender)}
          </span>
          {chip.count > 1 && <span className="text-tertiary tabular-nums">{numbers.format(chip.count)}</span>}
        </span>
      ))}
      {more && <span className="shrink-0 text-12 text-tertiary tabular-nums">{more}</span>}
    </span>
  );
  const chevron = <Chevron aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />;
  return (
    <div data-testid="bundle-row" data-bundle={bundle.key} data-expanded={expanded} className="h-full">
      <span className="sr-only" data-testid="row-name">
        {label}
      </span>
      <div
        aria-hidden
        className={cx(
          "flex h-full w-full gap-3 overflow-hidden rounded-row px-3 text-start transition-colors group-hover/row:bg-hover",
          narrow ? "items-start py-2" : "items-center",
        )}
      >
        <span aria-hidden className={cx("flex w-(--ui-icon-small) shrink-0 items-center justify-center", narrow && "h-5")}>
          {unread > 0 && <span className="size-(--ui-dot) rounded-full bg-accent" />}
        </span>
        <span
          aria-hidden
          className={cx("flex size-(--ui-avatar) shrink-0 items-center justify-center rounded-row", BUNDLE_TILES[category])}
        >
          <Icon className="size-(--ui-icon-small)" strokeWidth={1.75} />
        </span>
        {narrow ? (
          <span className="flex min-w-0 flex-1 flex-col gap-1">
            <span className="flex h-5 min-w-0 items-center gap-2">
              {title}
              <span className="flex-1" />
              {chevron}
            </span>
            {chips}
          </span>
        ) : (
          <>
            {title}
            {chips}
            {chevron}
          </>
        )}
      </div>
    </div>
  );
}
