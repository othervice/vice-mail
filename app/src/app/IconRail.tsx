import { CircleExclamationPoint, CloudOff, History, SendHorizontal, Settings, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { useRovingFocus } from "../components/useRovingFocus";
import { accountFill } from "../features/accounts/accountColor";
import { useAccountDragSource, useAccountMoveByDrag } from "../features/accounts/accountDrag";
import { movedOrder, useAccountArrange } from "../features/accounts/arrange";
import { useSelectedAccount } from "../features/accounts/selection";
import { useOutboxCounts } from "../features/actions/queries";
import { useOutgoing } from "../features/composer/queries";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { DROP_REFUSED_CLASS, dropTargetState, useDropTarget } from "../features/mailboxes/useDropTarget";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import { sidebarIconClass, sidebarRowClass } from "../features/layout/sidebarStyles";
import { VIEW_COMMANDS, type ViewCommandId } from "../features/actions/registry";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "../features/actions/shortcuts";
import { useAppInfo } from "../features/app-info/queries";
import { useShownColumnViews } from "../features/layout/columnViews";
import { useBoundShortcuts } from "../stores/shortcuts";
import type { Account } from "../services/ipc";
import { useUiStore, type MailViewId } from "../stores/ui";
import type { DropRole } from "../stores/drag";

/** Roving focus of the rail group the entry belongs to (`useRovingFocus`). */
type Roving = ReturnType<ReturnType<typeof useRovingFocus>["itemProps"]>;

/** The entries of the rail's bottom group (accounts only in the folded rail). */
const accountKey = (id: string) => `account-${id}`;
const ACTIVITY_KEY = "activity";
const OUTBOX_KEY = "outbox";
const SETTINGS_KEY = "settings";
const STATUS_KEYS = [ACTIVITY_KEY, OUTBOX_KEY, SETTINGS_KEY];

/**
 * Stage 17: the rail's fixed views around the ones the user chose for the column ("Edit views…"),
 * which stand between the inbox and Sent in the user's order.
 */
const RAIL_TOP = ["inbox"] as const satisfies readonly MailViewId[];
const RAIL_BOTTOM = ["sent", "drafts", "archive", "trash", "folders"] as const satisfies readonly MailViewId[];

interface RailButtonProps {
  icon: LucideIcon;
  label: string;
  /** Labelled sidebar: the name stands next to the icon instead of in a tooltip only. */
  expanded: boolean;
  active?: boolean;
  onClick: () => void;
  /** For the Folders entry, which unfolds the sidebar instead of opening a view. */
  disclosure?: { expanded: boolean; controls?: string };
  /** Small count or mark on the icon (activity). */
  badge?: ReactNode;
  /**
   * Visible name in the labelled sidebar when `label` (the accessible name) says more, e.g. a
   * count. `label` must start with this text (WCAG 2.5.3, label in name).
   */
  text?: string;
  /** In the labelled sidebar: shown at the end of the row instead of `badge` on the icon. */
  trailing?: ReactNode;
  /**
   * Rows dragged out of any list can be dropped here (stage 22, decision 0081): each goes to the
   * folder with this role in its own account.
   */
  dropRole?: DropRole | undefined;
  /** Stage 8: the view's shortcut, in the tooltip and for screen readers. */
  shortcut?: { hint: string; aria: string } | undefined;
  /** The group's single Tab stop and arrow keys. */
  roving: Roving;
  /** Stable hook for the desktop tests (labels change with the language). */
  "data-testid": string;
}

function RailButton({ icon: Icon, label, expanded, active = false, onClick, disclosure, badge, text, trailing, dropRole, shortcut, roving, ...rest }: RailButtonProps) {
  const { t } = useTranslation();
  const drop = useDropTarget(dropRole && { kind: "role", role: dropRole }, { id: `rail-${rest["data-testid"]}`, folder: label });
  const buttonRef = useRef<HTMLButtonElement>(null);
  // Design pass 23.11.2026: in a low window the upper group scrolls; the active view is scrolled
  // into it (also when a shortcut opens a view further down). Only that group moves, nothing else.
  useEffect(() => {
    const button = buttonRef.current;
    const scroller = button?.closest<HTMLElement>("[data-rail-scroll]");
    if (!active || !button || !scroller) return;
    const item = button.getBoundingClientRect();
    const area = scroller.getBoundingClientRect();
    if (item.top < area.top) scroller.scrollTop -= area.top - item.top;
    else if (item.bottom > area.bottom) scroller.scrollTop += item.bottom - area.bottom;
  }, [active]);
  // Active: accent icon on a soft fill plus a bar at the rail edge, so it is not colour alone. In
  // the column the rows look like the rows of its trees.
  const className = expanded
    ? sidebarRowClass(
        active,
        cx("w-full gap-2 ps-3 pe-2", drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent", drop.refused && DROP_REFUSED_CLASS),
      )
    : cx(
        "relative flex size-(--ui-rail-item) items-center justify-center rounded-row transition-colors",
        active
          ? "bg-accent-soft font-medium text-accent before:absolute before:inset-y-2 before:-left-2 before:w-1 before:rounded-control before:bg-accent-fill"
          : "text-secondary hover:bg-rail-hover hover:text-primary",
        drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
        drop.refused && DROP_REFUSED_CLASS,
      );
  const button = (
    <button
      ref={buttonRef}
      type="button"
      aria-label={label}
      aria-keyshortcuts={shortcut?.aria}
      aria-current={active ? "page" : undefined}
      aria-expanded={disclosure?.expanded}
      aria-controls={disclosure?.controls}
      onClick={onClick}
      {...roving}
      {...(dropRole && drop.handlers)}
      className={className}
      data-testid={rest["data-testid"]}
      data-drop-target={dropTargetState(drop)}
    >
      <span className="relative flex shrink-0">
        <Icon aria-hidden className={expanded ? sidebarIconClass(active) : "size-(--ui-icon) shrink-0"} strokeWidth={1.75} />
        {(!expanded || trailing === undefined) && badge}
      </span>
      {expanded && (
        <span aria-hidden className="truncate">
          {text ?? label}
        </span>
      )}
      {/* A space before the trailing count: name and number are two words, as in the name. */}
      {expanded && trailing !== undefined && (
        <>
          {" "}
          {trailing}
        </>
      )}
    </button>
  );
  return (
    <Tooltip
      label={shortcut ? t("actions.withShortcut", { label, shortcut: shortcut.hint }) : label}
      side="right"
      disabled={expanded}
    >
      {button}
    </Tooltip>
  );
}

/**
 * Small mark on an account that is offline or stopped, so the rail shows trouble at a glance: at
 * the end of the entry's lower edge, clear of the account's dot.
 */
function StateBadge({ account }: { account: Account }) {
  const state = account.status.state;
  if (state !== "offline" && state !== "error") return null;
  const Icon = state === "error" ? CircleExclamationPoint : CloudOff;
  return (
    <span className="absolute end-1 bottom-1 flex rounded-full bg-rail p-px" data-testid="account-state-badge">
      <Icon
        aria-hidden
        className={cx("size-3", state === "error" ? "text-danger" : "text-secondary")}
        strokeWidth={2}
      />
    </span>
  );
}

/**
 * The count of the activity and outbox entries: on the icon in the narrow rail, at the end of the
 * row in the labelled sidebar (so name and number read in the order they are shown).
 */
function RailCount({ value, attention, trailing = false, ...rest }: { value: string; attention: boolean; trailing?: boolean; "data-testid": string }) {
  return (
    <span
      aria-hidden
      data-testid={rest["data-testid"]}
      data-attention={attention || undefined}
      className={cx(
        "flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-12 leading-none font-semibold tabular-nums",
        trailing ? "ms-auto shrink-0" : "absolute -top-2 -right-2",
        attention ? "bg-danger text-on-danger" : "bg-rail text-secondary ring-1 ring-border-strong",
      )}
    >
      {value}
    </span>
  );
}

/**
 * The collected state of the outbox (docs/DESIGN.md: pending and failed actions visible but
 * quiet): a count on the activity icon, in the danger colour once something needs the user.
 */
function ActivityButton({ accounts, expanded, active, roving }: { accounts: readonly Account[]; expanded: boolean; active: boolean; roving: Roving }) {
  const { t, i18n } = useTranslation();
  const showActivity = useUiStore((state) => state.showActivity);
  const counts = useOutboxCounts(accounts);
  const pending = counts.reduce((sum, c) => sum + c.pending, 0);
  const attention = counts.reduce((sum, c) => sum + c.failed + c.conflict, 0);
  const numbers = new Intl.NumberFormat(i18n.language);
  // The name starts with what is visible: the view's name, then the number on the badge.
  const label =
    attention > 0
      ? t("activity.badgeAttention", { pending, attention })
      : pending > 0
        ? t("activity.badgeWaiting", { pending, attention })
        : t("activity.nav");
  const shown = attention > 0 ? attention : pending;
  const count = shown > 0 ? numbers.format(shown) : null;
  return (
    <RailButton
      icon={History}
      label={label}
      text={t("activity.nav")}
      expanded={expanded}
      active={active}
      onClick={showActivity}
      roving={roving}
      badge={count && <RailCount value={count} attention={attention > 0} data-testid="activity-badge" />}
      trailing={count ? <RailCount value={count} attention={attention > 0} data-testid="activity-badge" trailing /> : undefined}
      data-testid="rail-activity"
    />
  );
}

/** Stage 5: the outbox ("Ausgang"); red count while sends failed or need an answer. */
function OutboxButton({ accounts, expanded, active, roving }: { accounts: readonly Account[]; expanded: boolean; active: boolean; roving: Roving }) {
  const { t, i18n } = useTranslation();
  const showOutbox = useUiStore((state) => state.showOutbox);
  const mails = useOutgoing(accounts).flatMap((result) => result.data ?? []);
  const attention = mails.filter((m) => m.state === "failed" || m.state === "unclear").length;
  const pending = mails.filter((m) => m.state !== "sent" && m.state !== "failed" && m.state !== "unclear").length;
  const shown = attention > 0 ? attention : pending;
  const count = shown > 0 ? new Intl.NumberFormat(i18n.language).format(shown) : null;
  // The name starts with what is visible: the view's name, then the number on the badge.
  const label =
    attention > 0
      ? t("composer.outbox.badgeAttention", { attention, pending })
      : pending > 0
        ? t("composer.outbox.badgeWaiting", { attention, pending })
        : t("composer.outbox.nav");
  return (
    <RailButton
      icon={SendHorizontal}
      label={label}
      text={t("composer.outbox.nav")}
      expanded={expanded}
      active={active}
      onClick={showOutbox}
      roving={roving}
      badge={count && <RailCount value={count} attention={attention > 0} data-testid="outbox-badge" />}
      trailing={count ? <RailCount value={count} attention={attention > 0} data-testid="outbox-badge" trailing /> : undefined}
      data-testid="rail-outbox"
    />
  );
}

function useAccountLabel() {
  const { t } = useTranslation();
  return (account: Account) =>
    t("nav.accountWithState", {
      name: account.displayName || account.email,
      email: account.email,
      state: t(`nav.accountState.${account.status.state}`),
    });
}

/** One account's dot in the rail: picks the account, and (stage 23) is dragged to put it elsewhere. */
function AccountDot({ account, accounts, current, roving, line, dimmed, dropItem, onMove }: {
  account: Account;
  accounts: readonly Account[];
  current: boolean;
  roving: Roving;
  line: "before" | "after" | null;
  dimmed: boolean;
  dropItem: Record<string, string>;
  onMove: (step: -1 | 1) => void;
}) {
  const { t } = useTranslation();
  const selectAccount = useUiStore((state) => state.selectAccount);
  const label = useAccountLabel()(account);
  const dragSource = useAccountDragSource(account, accounts.length > 1);
  return (
    <li
      {...dropItem}
      data-account-drop={line ?? undefined}
      className={cx(
        "relative",
        // The place it goes: a line in the gap above or below the dot.
        line === "before" && "before:absolute before:inset-x-1 before:-top-1 before:h-1 before:rounded-full before:bg-accent-fill",
        line === "after" && "after:absolute after:inset-x-1 after:-bottom-1 after:h-1 after:rounded-full after:bg-accent-fill",
      )}
    >
      <Tooltip label={label} side="right">
        <button
          type="button"
          aria-label={label}
          aria-pressed={current}
          aria-description={accounts.length > 1 ? t("accountEdit.railDragHint") : undefined}
          aria-keyshortcuts={accounts.length > 1 ? "Alt+ArrowUp Alt+ArrowDown" : undefined}
          onClick={() => {
            selectAccount(account.id);
          }}
          onKeyDown={(event) => {
            if (!event.altKey || event.ctrlKey || event.metaKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
            event.preventDefault();
            onMove(event.key === "ArrowUp" ? -1 : 1);
          }}
          {...roving}
          {...dragSource}
          data-testid={`account-switch-${account.id}`}
          data-account-dragged={dimmed || undefined}
          // Which account the views show, not which view is open: a fill, no active bar.
          // With a single account there is nothing to tell apart.
          className={cx(
            "relative flex size-(--ui-rail-item) items-center justify-center rounded-row transition-colors",
            current && accounts.length > 1 ? "bg-selected" : "hover:bg-rail-hover",
            dimmed && "opacity-50",
          )}
        >
          <span aria-hidden className={cx("size-(--ui-rail-account-dot) rounded-full", accountFill(account.color))} data-testid="account-dot" />
          <StateBadge account={account} />
        </button>
      </Tooltip>
    </li>
  );
}

/**
 * One entry per account: a dot in its colour (stage 22, decision 0072: no initials), its name as
 * tooltip and accessible name, which always say what the colour alone cannot (two accounts may
 * share a colour). Stage 23: a dot dragged up or down puts the account there (a line shows where),
 * as in the column; Alt+Up and Alt+Down do the same from the keyboard.
 */
function AccountSwitcher({ accounts, selected, roving }: { accounts: readonly Account[]; selected: Account | null; roving: (key: string) => Roving }) {
  const { t } = useTranslation();
  const arrange = useAccountArrange();
  const drop = useAccountMoveByDrag(accounts);
  const [moved, setMoved] = useState("");
  const move = (account: Account, step: -1 | 1) => {
    const order = movedOrder(accounts, account.id, step);
    if (!order) return;
    void arrange.reorder(order).then((ok) => {
      if (ok) setMoved(t("accountEdit.moved", { name: account.displayName || account.email, position: order.indexOf(account.id) + 1, count: order.length }));
    });
  };
  return (
    <>
      <ul aria-label={t("nav.accounts")} {...drop.zoneProps} className="flex flex-col items-center gap-1" data-testid="rail-accounts">
        {accounts.map((account) => (
          <AccountDot
            key={account.id}
            account={account}
            accounts={accounts}
            current={account.id === selected?.id}
            roving={roving(accountKey(account.id))}
            line={drop.lineOf(account.id)}
            dimmed={drop.dragged === account.id}
            dropItem={drop.itemProps(account.id)}
            onMove={(step) => {
              move(account, step);
            }}
          />
        ))}
      </ul>
      <span aria-live="polite" className="sr-only" data-testid="rail-accounts-announce">
        {drop.announcement || moved}
      </span>
    </>
  );
}

/**
 * The bottom group of the rail and of the column: activity, outbox and settings with their counts
 * (in the narrow rail also the account switcher). One Tab stop, arrow keys inside.
 */
export function StatusGroup({ accounts, expanded }: { accounts: readonly Account[]; expanded: boolean }) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showSettings = useUiStore((state) => state.showSettings);
  const { account: selected } = useSelectedAccount();
  const bottomActive = view.kind === "activity" || view.kind === "outbox" || view.kind === "settings" ? view.kind : null;
  const bottom = useRovingFocus({
    keys: [...(expanded ? [] : accounts.map((account) => accountKey(account.id))), ...STATUS_KEYS],
    activeKey: bottomActive ?? (!expanded && selected ? accountKey(selected.id) : null),
  });
  const activityRoving = bottom.itemProps(ACTIVITY_KEY);
  const outboxRoving = bottom.itemProps(OUTBOX_KEY);
  const settingsRoving = bottom.itemProps(SETTINGS_KEY);
  // A hairline above the group: where the views scroll under it in a low window.
  return (
    <div
      role="toolbar"
      aria-label={t("nav.statusGroup")}
      {...bottom.groupProps}
      className={cx("flex shrink-0 flex-col border-t border-border-subtle py-3", expanded ? "gap-1 px-2" : "items-center gap-2")}
    >
      {/* Account switcher: one entry per account with its colour and name. Empty without accounts. */}
      {expanded ? null : <AccountSwitcher accounts={accounts} selected={selected} roving={bottom.itemProps} />}
      <ActivityButton accounts={accounts} expanded={expanded} active={view.kind === "activity"} roving={activityRoving} />
      <OutboxButton accounts={accounts} expanded={expanded} active={view.kind === "outbox"} roving={outboxRoving} />
      <RailButton
        icon={Settings}
        label={t("nav.settings")}
        expanded={expanded}
        active={view.kind === "settings"}
        onClick={() => {
          showSettings();
        }}
        roving={settingsRoving}
        data-testid="rail-settings"
      />
    </div>
  );
}

/**
 * The 56 px icon rail, shown while the accounts-and-folders column is folded (docs/DESIGN.md):
 * mail views on top, account switcher, activity, outbox and settings at the bottom. "Folders"
 * opens the column (in a narrow window over the content, `SidebarColumn`). Stage 17: the views
 * of the column's "Views" section show here as the user chose them, between the inbox and Sent.
 */
export function IconRail() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const overlay = useUiStore((state) => state.sidebarOverlay);
  const showMailView = useUiStore((state) => state.showMailView);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const dragRegion = useWindowDragRegion();
  const { accounts } = useSelectedAccount();
  const accountList = accounts.data ?? [];
  const chosen = useShownColumnViews();
  const viewIds: MailViewId[] = [...RAIL_TOP, ...chosen, ...RAIL_BOTTOM];
  /**
   * Rail views that take dragged rows (stage 22, decision 0081): each row goes to the folder with
   * that role in its own account, so rows of "All inboxes" never cross accounts here.
   */
  const dropRole = (id: MailViewId): DropRole | undefined => (id === "inbox" || id === "archive" || id === "trash" ? id : undefined);
  // Stage 8: Cmd/Ctrl+1 to 8 open the views (as the user assigned them).
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const shortcutsOf = useBoundShortcuts(platform);
  const commandShortcut = (command: ViewCommandId | "toggleSidebar") => {
    const first = shortcutsOf(command)[0];
    return first && { hint: formatShortcut(first, platform), aria: ariaShortcut(first, platform) };
  };
  const viewShortcut = (id: MailViewId) => {
    const command = (Object.keys(VIEW_COMMANDS) as ViewCommandId[]).find((key) => VIEW_COMMANDS[key] === id);
    return command ? commandShortcut(command) : undefined;
  };
  // Stage 8, keyboard: the views and the bottom group are one Tab stop each (arrow keys inside),
  // so Tab goes rail views, bottom group, list instead of through every entry.
  const views = useRovingFocus({ keys: viewIds, activeKey: view.kind === "mail" ? view.mailView : null });

  return (
    <nav
      aria-label={t("nav.label")}
      // While the column lies over the content, the rail under it is out of reach.
      inert={overlay || undefined}
      // Decision 0031: the darkest tonal step of the frame, a hairline to the list.
      className="flex h-full w-(--ui-rail-width) shrink-0 flex-col items-center border-r border-border-subtle bg-rail"
      data-testid="icon-rail"
      // Decision 0079: chrome; Mica shows through it on Windows 11.
      data-chrome
    >
      {/* Room for the window buttons that macOS draws over this corner. */}
      <div aria-hidden data-tauri-drag-region={dragRegion ? "deep" : undefined} className="h-(--ui-titlebar-inset) w-full shrink-0" />
      {/* Design pass 23.11.2026: in a low window this group scrolls, never sideways. Its edges fade
          within the 12 px padding, so an entry cut off at the edge reads as "more". The narrow rail
          shows no scroll bar (on Windows it would push the icons off centre); wheel, trackpad and
          Tab reach every entry, and the active view is scrolled into view. */}
      <div
        data-rail-scroll
        className={cx(
          "flex min-h-0 w-full flex-1 flex-col items-center overflow-x-hidden overflow-y-auto py-3 [scrollbar-width:none]",
          "[mask-image:linear-gradient(to_bottom,transparent,black_var(--ui-space-2),black_calc(100%-var(--ui-space-2)),transparent)]",
        )}
      >
        {/* Stage 8, keyboard: one Tab stop for the views, Up and Down move between them. */}
        <div role="toolbar" aria-label={t("nav.viewGroup")} {...views.groupProps} className="flex w-full flex-col">
          <ul className="flex w-full flex-col items-center gap-1">
            {viewIds.map((id) => (
              <li key={id}>
                {id === "folders" ? (
                  <RailButton
                    icon={MAIL_VIEW_ICONS[id]}
                    label={t("nav.folders")}
                    expanded={false}
                    disclosure={{ expanded: overlay }}
                    onClick={toggleSidebar}
                    shortcut={commandShortcut("toggleSidebar")}
                    roving={views.itemProps(id)}
                    data-testid={`rail-${id}`}
                  />
                ) : (
                  <RailButton
                    icon={MAIL_VIEW_ICONS[id]}
                    label={t(`nav.${id}`)}
                    expanded={false}
                    active={view.kind === "mail" && view.mailView === id}
                    onClick={() => {
                      showMailView(id);
                    }}
                    dropRole={dropRole(id)}
                    shortcut={viewShortcut(id)}
                    roving={views.itemProps(id)}
                    data-testid={`rail-${id}`}
                  />
                )}
              </li>
            ))}
          </ul>
        </div>
      </div>
      <StatusGroup accounts={accountList} expanded={false} />
    </nav>
  );
}
