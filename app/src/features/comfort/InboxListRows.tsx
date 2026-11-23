import { AlarmClockCheck, Bell, ChevronDown, ChevronRight, Newspaper, Pin, Zap, type LucideIcon } from "lucide-react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { Address, MessageCategory } from "../../services/ipc";
import type { InboxBlock, InboxBundle } from "./inboxRows";

const BLOCK_ICONS: Record<InboxBlock, LucideIcon> = {
  pinned: Pin,
  returned: AlarmClockCheck,
  priority: Zap,
};

export const BUNDLE_ICONS: Record<Exclude<MessageCategory, "person">, LucideIcon> = {
  notification: Bell,
  newsletter: Newspaper,
};

/**
 * Enter and Space belong to the button inside the list, not to the list's own keys (open,
 * select) or the window's shortcuts.
 */
function keepKeys(event: KeyboardEvent) {
  if (event.key === "Enter" || event.key === " ") event.stopPropagation();
}

function senderName(address: Address): string {
  return address.name?.trim() || address.email;
}

/** Heading of a block above the dated rows; "Priority" on its tinted surface (docs/DESIGN.md). */
export function BlockHeading({ block }: { block: InboxBlock }) {
  const { t } = useTranslation();
  const Icon = BLOCK_ICONS[block];
  return (
    <div
      aria-hidden
      data-testid={`inbox-block-${block}`}
      className={cx(
        "flex h-full items-end gap-2 px-3 pb-1 text-12 font-semibold",
        block === "priority" ? "rounded-t-card bg-priority-tint text-primary" : "text-secondary",
      )}
    >
      <Icon className="mb-0.5 size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
      <span className="truncate">{t(`comfort.blocks.${block}`)}</span>
    </div>
  );
}

/** "Show all (n)" below the priority block: the important senders' messages as their own list. */
export function ShowAllRow({ total, unread, onShow }: { total: number; unread: number; onShow: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full items-center rounded-b-card bg-priority-tint px-3">
      <button
        type="button"
        onClick={onShow}
        onKeyDown={keepKeys}
        data-testid="priority-show-all"
        className="rounded-control px-1 text-12 font-medium text-accent hover:underline focus-visible:outline-2 focus-visible:outline-focus-ring"
      >
        {unread > 0 ? t("comfort.priority.showAllUnread", { count: total, unread }) : t("comfort.priority.showAll", { count: total })}
      </button>
    </div>
  );
}

interface BundleRowProps {
  bundle: InboxBundle;
  expanded: boolean;
  onToggle: () => void;
}

/**
 * Notifications or newsletters of one date section as one row: symbol, name, counter and chips
 * of the most frequent senders; a click (or Enter) unfolds the members in place below it.
 */
export function BundleRow({ bundle, expanded, onToggle }: BundleRowProps) {
  const { t } = useTranslation();
  const category = bundle.category === "person" ? "notification" : bundle.category;
  const Icon = BUNDLE_ICONS[category];
  const name = t(`comfort.bundle.${category}`);
  const { count, unread, senders } = bundle.summary;
  const names = senders.map((chip) => senderName(chip.sender));
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <button
      type="button"
      aria-expanded={expanded}
      aria-label={t("comfort.bundle.label", {
        name,
        count,
        unread,
        senders: names.join(", "),
      })}
      onClick={onToggle}
      onKeyDown={keepKeys}
      data-testid="bundle-row"
      data-bundle={bundle.key}
      className="group/bundle flex h-full w-full items-center gap-3 rounded-card px-3 text-start hover:bg-hover focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus-ring"
    >
      <span aria-hidden className="flex size-(--ui-icon-small) shrink-0 items-center justify-center">
        {unread > 0 && <span className="size-(--ui-dot) rounded-full bg-accent" />}
      </span>
      <span
        aria-hidden
        className="flex size-(--ui-avatar) shrink-0 items-center justify-center rounded-full bg-accent-soft text-accent"
      >
        <Icon className="size-(--ui-icon-small)" strokeWidth={1.75} />
      </span>
      <span aria-hidden className={cx("shrink-0 text-14 text-primary", unread > 0 ? "font-semibold" : "font-regular")}>
        {name}
      </span>
      <span aria-hidden className="shrink-0 text-13 text-secondary tabular-nums" data-testid="bundle-count">
        {unread > 0 ? t("comfort.bundle.counter", { count, unread }) : count}
      </span>
      <span aria-hidden className="flex min-w-0 flex-1 items-center gap-1 overflow-hidden">
        {senders.map((chip) => (
          <span
            key={chip.sender.email}
            title={chip.sender.email}
            data-testid="bundle-chip"
            className={cx(
              "min-w-0 shrink truncate rounded-full border border-border px-2 text-12",
              chip.unread > 0 ? "font-medium text-primary" : "text-secondary",
            )}
          >
            {senderName(chip.sender)}
            {chip.count > 1 && <span className="ps-1 text-tertiary tabular-nums">{chip.count}</span>}
          </span>
        ))}
      </span>
      <Chevron aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
    </button>
  );
}
