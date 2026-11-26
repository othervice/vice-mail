import { CircleExclamationPoint, CloudOff, History, SendHorizontal, Settings, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { useRovingFocus } from "../components/useRovingFocus";
import { accountFill } from "../features/accounts/accountColor";
import { accountUnread } from "../features/mailboxes/MailboxTree";
import { useMailboxes } from "../features/mailboxes/queries";
import { useAccountDragSource, useAccountMoveByDrag } from "../features/accounts/accountDrag";
import { movedOrder, useAccountArrange } from "../features/accounts/arrange";
import { useSelectedAccount } from "../features/accounts/selection";
import { useOutboxCounts } from "../features/actions/queries";
import { useOutgoing } from "../features/composer/queries";
import { DROP_REFUSED_CLASS, dropTargetState, useDropTarget } from "../features/mailboxes/useDropTarget";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import { VIEW_COMMANDS, type ViewCommandId } from "../features/actions/registry";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "../features/actions/shortcuts";
import { useAppInfo } from "../features/app-info/queries";
import { useColumnGroups } from "../features/layout/columnViews";
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
 * Stage 17: the rail's fixed views around the ones the user chose for the column ("Edit views…").
 * Stage 23 (decision 0085): grouped as in the column: the inbox, the views of "All accounts" and
 * Sent; a hairline; the views under "More", then Drafts, Archive, Trash and "Folders".
 */
const RAIL_TOP = ["inbox"] as const satisfies readonly MailViewId[];
const RAIL_MAIN_END = ["sent"] as const satisfies readonly MailViewId[];
const RAIL_BOTTOM = ["drafts", "archive", "trash", "folders"] as const satisfies readonly MailViewId[];

interface RailButtonProps {
  icon: LucideIcon;
  label: string;
  active?: boolean;
  onClick: () => void;
  /** For the Folders entry, which unfolds the sidebar instead of opening a view. */
  disclosure?: { expanded: boolean; controls?: string };
  /** Small count or mark on the icon (activity). */
  badge?: ReactNode;
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

function RailButton({ icon: Icon, label, active = false, onClick, disclosure, badge, dropRole, shortcut, roving, ...rest }: RailButtonProps) {
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
  // Stage 23 (decision 0085): the active view on the column's selected fill with its icon in the
  // accent, as in the column; no bar at the rail's edge.
  const className = cx(
    "relative flex size-(--ui-rail-item) items-center justify-center rounded-tile transition-colors",
    active ? "bg-rail-selected text-accent" : "text-secondary hover:bg-rail-hover hover:text-primary",
    drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
    drop.refused && DROP_REFUSED_CLASS,
  );
  return (
    <Tooltip label={shortcut ? t("actions.withShortcut", { label, shortcut: shortcut.hint }) : label} side="right">
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
          <Icon aria-hidden className="size-(--ui-icon) shrink-0" strokeWidth={1.75} />
          {badge}
        </span>
      </button>
    </Tooltip>
  );
}

/**
 * Small mark on an account that is offline or stopped, so the rail shows trouble at a glance: at
 * the end of the entry's upper edge, clear of the account's dot and (stage 23) its count below it.
 */
function StateBadge({ account }: { account: Account }) {
  const state = account.status.state;
  if (state !== "offline" && state !== "error") return null;
  const Icon = state === "error" ? CircleExclamationPoint : CloudOff;
  return (
    <span className="absolute end-1 top-1 flex rounded-full bg-rail p-px" data-testid="account-state-badge">
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
 * row in the column (so name and number read in the order they are shown), in the danger colour
 * once something needs the user.
 */
export function RailCount({ value, attention, trailing = false, ...rest }: { value: string; attention: boolean; trailing?: boolean; "data-testid": string }) {
  return (
    <span
      aria-hidden
      data-testid={rest["data-testid"]}
      data-attention={attention || undefined}
      className={cx(
        "flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-11 leading-none font-semibold tabular-nums",
        trailing ? "ms-auto shrink-0" : "absolute -top-2 -right-2",
        attention ? "bg-danger text-on-danger" : "bg-rail text-secondary ring-1 ring-border-strong",
      )}
    >
      {value}
    </span>
  );
}

/** What the activity entry shows and says: its count and name (`attention` in the danger colour). */
export interface StatusEntry {
  pending: number;
  attention: number;
  /** The accessible name: the view's name, then the number on the badge. */
  label: string;
  /** The view's name alone. */
  text: string;
  /** The number shown (what needs the user, else what waits), null for none. */
  count: string | null;
}

/**
 * The collected state of the outbox (docs/DESIGN.md: pending and failed actions visible but
 * quiet): a count on the activity icon, in the danger colour once something needs the user.
 */
export function useActivityEntry(accounts: readonly Account[]): StatusEntry {
  const { t, i18n } = useTranslation();
  const counts = useOutboxCounts(accounts);
  const pending = counts.reduce((sum, c) => sum + c.pending, 0);
  const attention = counts.reduce((sum, c) => sum + c.failed + c.conflict, 0);
  const label =
    attention > 0
      ? t("activity.badgeAttention", { pending, attention })
      : pending > 0
        ? t("activity.badgeWaiting", { pending, attention })
        : t("activity.nav");
  const shown = attention > 0 ? attention : pending;
  return { pending, attention, label, text: t("activity.nav"), count: shown > 0 ? new Intl.NumberFormat(i18n.language).format(shown) : null };
}

/** Stage 5: the outbox ("Ausgang"); red count while sends failed or need an answer. */
export function useOutboxEntry(accounts: readonly Account[]): StatusEntry {
  const { t, i18n } = useTranslation();
  const mails = useOutgoing(accounts).flatMap((result) => result.data ?? []);
  const attention = mails.filter((m) => m.state === "failed" || m.state === "unclear").length;
  const pending = mails.filter((m) => m.state !== "sent" && m.state !== "failed" && m.state !== "unclear").length;
  const shown = attention > 0 ? attention : pending;
  const label =
    attention > 0
      ? t("composer.outbox.badgeAttention", { attention, pending })
      : pending > 0
        ? t("composer.outbox.badgeWaiting", { attention, pending })
        : t("composer.outbox.nav");
  return { pending, attention, label, text: t("composer.outbox.nav"), count: shown > 0 ? new Intl.NumberFormat(i18n.language).format(shown) : null };
}

function ActivityButton({ accounts, active, roving }: { accounts: readonly Account[]; active: boolean; roving: Roving }) {
  const showActivity = useUiStore((state) => state.showActivity);
  const entry = useActivityEntry(accounts);
  return (
    <RailButton
      icon={History}
      label={entry.label}
      active={active}
      onClick={showActivity}
      roving={roving}
      badge={entry.count && <RailCount value={entry.count} attention={entry.attention > 0} data-testid="activity-badge" />}
      data-testid="rail-activity"
    />
  );
}

function OutboxButton({ accounts, active, roving }: { accounts: readonly Account[]; active: boolean; roving: Roving }) {
  const showOutbox = useUiStore((state) => state.showOutbox);
  const entry = useOutboxEntry(accounts);
  return (
    <RailButton
      icon={SendHorizontal}
      label={entry.label}
      active={active}
      onClick={showOutbox}
      roving={roving}
      badge={entry.count && <RailCount value={entry.count} attention={entry.attention > 0} data-testid="outbox-badge" />}
      data-testid="rail-outbox"
    />
  );
}

/** The most unread mail the count under an account's dot shows exactly; more reads "999+". */
const RAIL_UNREAD_MAX = 999;

/** The count under an account's dot, as shown: "999+" from 1,000 on. */
function railUnread(unread: number, language: string): string {
  const format = new Intl.NumberFormat(language);
  return unread > RAIL_UNREAD_MAX ? `${format.format(RAIL_UNREAD_MAX)}+` : format.format(unread);
}

function useAccountLabel() {
  const { t, i18n } = useTranslation();
  return (account: Account, unread: number) => {
    const label = t("nav.accountWithState", {
      name: account.displayName || account.email,
      email: account.email,
      state: t(`nav.accountState.${account.status.state}`),
    });
    if (unread <= 0) return label;
    // The name says the count as the dot shows it (WCAG 2.5.3: the visible text is in the name).
    return unread > RAIL_UNREAD_MAX
      ? t("sidebar.unreadOver", { name: label, shown: railUnread(unread, i18n.language) })
      : t("sidebar.folderUnread", { name: label, count: unread });
  };
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
  const { t, i18n } = useTranslation();
  const selectAccount = useUiStore((state) => state.selectAccount);
  // Stage 23 (decision 0085): the account's unread mail (inbox and own folders, as its section in
  // the column counts it) on its dot.
  const unread = accountUnread(useMailboxes(account.id).data ?? []);
  const label = useAccountLabel()(account, unread);
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
            "relative flex size-(--ui-rail-item) items-center justify-center rounded-tile transition-colors",
            current && accounts.length > 1 ? "bg-rail-selected" : "hover:bg-rail-hover",
            dimmed && "opacity-50",
          )}
        >
          {/* Stage 23 (decision 0085): the count under the dot, so it never covers the colour. */}
          <span className="flex flex-col items-center gap-1">
            <span aria-hidden className={cx("size-(--ui-rail-account-dot) rounded-full", accountFill(account.color))} data-testid="account-dot" />
            {unread > 0 && (
              <span aria-hidden className="text-11 leading-none font-medium text-secondary tabular-nums" data-testid={`account-count-${account.id}`}>
                {railUnread(unread, i18n.language)}
              </span>
            )}
          </span>
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
 * The bottom group of the narrow rail: the account switcher, activity, outbox and settings with
 * their counts. One Tab stop, arrow keys inside. (The column has its own foot, `SidebarFooter`.)
 */
function StatusGroup({ accounts }: { accounts: readonly Account[] }) {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showSettings = useUiStore((state) => state.showSettings);
  const { account: selected } = useSelectedAccount();
  const bottomActive = view.kind === "activity" || view.kind === "outbox" || view.kind === "settings" ? view.kind : null;
  const bottom = useRovingFocus({
    keys: [...accounts.map((account) => accountKey(account.id)), ...STATUS_KEYS],
    activeKey: bottomActive ?? (selected ? accountKey(selected.id) : null),
  });
  // A hairline above the group: where the views scroll under it in a low window.
  return (
    <div
      role="toolbar"
      aria-label={t("nav.statusGroup")}
      {...bottom.groupProps}
      className="flex shrink-0 flex-col items-center gap-2 border-t border-border-subtle py-3"
    >
      {/* Account switcher: one entry per account with its colour and name. Empty without accounts. */}
      <AccountSwitcher accounts={accounts} selected={selected} roving={bottom.itemProps} />
      <ActivityButton accounts={accounts} active={view.kind === "activity"} roving={bottom.itemProps(ACTIVITY_KEY)} />
      <OutboxButton accounts={accounts} active={view.kind === "outbox"} roving={bottom.itemProps(OUTBOX_KEY)} />
      <RailButton
        icon={Settings}
        label={t("nav.settings")}
        active={view.kind === "settings"}
        onClick={() => {
          showSettings();
        }}
        roving={bottom.itemProps(SETTINGS_KEY)}
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
 * Stage 23 (decision 0085): grouped like the column (inbox, the views of "All accounts", Sent; the
 * views under "More", Drafts, Archive, Trash, Folders), the accounts with their unread counts.
 */
export function IconRail() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const overlay = useUiStore((state) => state.sidebarOverlay);
  const showMailView = useUiStore((state) => state.showMailView);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const { accounts } = useSelectedAccount();
  const accountList = accounts.data ?? [];
  const groups = useColumnGroups();
  const mainIds: MailViewId[] = [...RAIL_TOP, ...groups.main, ...RAIL_MAIN_END];
  const restIds: MailViewId[] = [...groups.more, ...RAIL_BOTTOM];
  const viewIds: MailViewId[] = [...mainIds, ...restIds];
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
      {/* Stage 23 (decision 0084): the window buttons sit in the toolbar across the window. */}
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
          {[mainIds, restIds].map((ids, group) => (
            <ul
              key={group}
              // Stage 23 (decision 0085): a hairline between the views of "All accounts" and the rest.
              className={cx("flex w-full flex-col items-center gap-1", group > 0 && "mt-2 border-t border-border-subtle pt-2")}
              data-testid={group === 0 ? "icon-rail-views-main" : "icon-rail-views-more"}
            >
              {ids.map((id) => (
                <li key={id}>
                  {id === "folders" ? (
                    <RailButton
                      icon={MAIL_VIEW_ICONS[id]}
                      label={t("nav.folders")}
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
          ))}
        </div>
      </div>
      <StatusGroup accounts={accountList} />
    </nav>
  );
}
