import { CircleExclamationPoint, CloudOff, History, SendHorizontal, Settings, type LucideIcon } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { accountFill } from "../features/accounts/accountColor";
import { Avatar } from "../features/accounts/Avatar";
import { useSelectedAccount } from "../features/accounts/selection";
import { useOutboxCounts } from "../features/actions/queries";
import { useOutgoing } from "../features/composer/queries";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { MailboxTree } from "../features/mailboxes/MailboxTree";
import { UnifiedTree } from "../features/unified/UnifiedTree";
import { FavoriteFolders } from "../features/organize/folders";
import { SavedSearchNav } from "../features/search/SavedSearchNav";
import { useMailboxes } from "../features/mailboxes/queries";
import { isRoleView, mailboxWithRole } from "../features/mailboxes/tree";
import { useDropTarget } from "../features/mailboxes/useDropTarget";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import { VIEW_COMMANDS, type ViewCommandId } from "../features/actions/registry";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "../features/actions/shortcuts";
import { useAppInfo } from "../features/app-info/queries";
import { useBoundShortcuts } from "../stores/shortcuts";
import type { Account, Mailbox, MailboxId } from "../services/ipc";
import { MAIL_VIEW_IDS, useUiStore } from "../stores/ui";

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
  /** Rows dragged out of the list can be dropped here: they move into this folder. */
  dropMailbox?: Mailbox | undefined;
  /** Stage 8: the view's shortcut, in the tooltip and for screen readers. */
  shortcut?: { hint: string; aria: string } | undefined;
  /** Stable hook for the desktop tests (labels change with the language). */
  "data-testid": string;
}

function RailButton({ icon: Icon, label, expanded, active = false, onClick, disclosure, badge, text, trailing, dropMailbox, shortcut, ...rest }: RailButtonProps) {
  const { t } = useTranslation();
  const drop = useDropTarget(dropMailbox);
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
  // Active: accent icon on a soft fill plus a bar at the rail edge, so it is not colour alone.
  const className = cx(
    "relative flex items-center rounded-row transition-colors",
    expanded ? "h-8 w-full gap-3 px-3 text-13" : "size-(--ui-rail-item) justify-center",
    active
      ? "bg-accent-soft font-medium text-accent before:absolute before:inset-y-2 before:-left-2 before:w-1 before:rounded-control before:bg-accent-fill"
      : "text-secondary hover:bg-rail-hover hover:text-primary",
    drop.over && "bg-accent-soft text-primary outline-2 -outline-offset-2 outline-accent",
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
      {...(dropMailbox && drop.handlers)}
      className={className}
      data-testid={rest["data-testid"]}
      data-drop-target={drop.over ? "over" : drop.accepts ? "ready" : undefined}
    >
      <span className="relative flex shrink-0">
        <Icon aria-hidden className="size-(--ui-icon) shrink-0" strokeWidth={1.75} />
        {(!expanded || trailing === undefined) && badge}
      </span>
      {expanded && (
        <span aria-hidden className="truncate">
          {text ?? label}
        </span>
      )}
      {expanded && trailing}
    </button>
  );
  // Same element in both modes: unfolding the sidebar keeps the focus on the button.
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

/** Small mark on an account that is offline or stopped, so the rail shows trouble at a glance. */
function StateBadge({ account }: { account: Account }) {
  const state = account.status.state;
  if (state !== "offline" && state !== "error") return null;
  const Icon = state === "error" ? CircleExclamationPoint : CloudOff;
  return (
    <span className="absolute -right-1 -bottom-1 flex rounded-full bg-rail p-px">
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
function ActivityButton({ accounts, expanded, active }: { accounts: readonly Account[]; expanded: boolean; active: boolean }) {
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
      badge={count && <RailCount value={count} attention={attention > 0} data-testid="activity-badge" />}
      trailing={count ? <RailCount value={count} attention={attention > 0} data-testid="activity-badge" trailing /> : undefined}
      data-testid="rail-activity"
    />
  );
}

/** Stage 5: the outbox ("Ausgang"); red count while sends failed or need an answer. */
function OutboxButton({ accounts, expanded, active }: { accounts: readonly Account[]; expanded: boolean; active: boolean }) {
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

/** One entry per account with its colour, initials and name (tooltip and accessible name). */
function AccountSwitcher({ accounts, selected }: { accounts: readonly Account[]; selected: Account | null }) {
  const { t } = useTranslation();
  const selectAccount = useUiStore((state) => state.selectAccount);
  const accountLabel = useAccountLabel();
  return (
    <ul aria-label={t("nav.accounts")} className="flex flex-col items-center gap-1">
      {accounts.map((account) => {
        const label = accountLabel(account);
        const current = account.id === selected?.id;
        return (
          <li key={account.id}>
            <Tooltip label={label} side="right">
              <button
                type="button"
                aria-label={label}
                aria-pressed={current}
                onClick={() => {
                  selectAccount(account.id);
                }}
                data-testid={`account-switch-${account.id}`}
                // Which account the views show, not which view is open: a fill, no active bar.
                // With a single account there is nothing to tell apart.
                className={cx(
                  "relative flex size-(--ui-rail-item) items-center justify-center rounded-row transition-colors",
                  current && accounts.length > 1 ? "bg-selected" : "hover:bg-rail-hover",
                )}
              >
                <span className="relative flex">
                  <Avatar name={account.displayName} email={account.email} accountColor={account.color} />
                  <StateBadge account={account} />
                </span>
              </button>
            </Tooltip>
          </li>
        );
      })}
    </ul>
  );
}

/** Folder tree of one account in the unfolded sidebar, headed by the account's name. */
function AccountFolders({ account, selected }: { account: Account; selected: boolean }) {
  const view = useUiStore((state) => state.view);
  const selectAccount = useUiStore((state) => state.selectAccount);
  const mailboxes = useMailboxes(account.id);
  const accountLabel = useAccountLabel();

  let current: MailboxId | null = null;
  if (view.kind === "mailbox" && view.accountId === account.id) current = view.mailboxId;
  // Stage 7: while the unified inbox is shown, no account's own inbox is the current folder.
  const unified = useUiStore((state) => state.unifiedInbox !== null);
  if (view.kind === "mail" && selected && isRoleView(view.mailView) && !(unified && view.mailView === "inbox")) {
    current = mailboxWithRole(mailboxes.data ?? [], view.mailView)?.id ?? null;
  }

  return (
    <section aria-label={accountLabel(account)} className="flex flex-col gap-1">
      <button
        type="button"
        aria-pressed={selected}
        onClick={() => {
          selectAccount(account.id);
        }}
        className={cx(
          "flex h-8 items-center gap-2 rounded-control px-2 text-12 font-semibold transition-colors",
          selected ? "text-primary" : "text-secondary hover:bg-rail-hover hover:text-primary",
        )}
      >
        <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
        <span className="truncate">{account.displayName || account.email}</span>
        <span className="relative ms-auto flex size-3">
          <StateBadge account={account} />
        </span>
      </button>
      <MailboxTree account={account} currentMailboxId={current} />
    </section>
  );
}

/**
 * The 56 px icon rail: mail views on top, account switcher and settings at the bottom. "Folders"
 * unfolds it into a labelled sidebar with the folder tree of every account (docs/DESIGN.md).
 */
export function IconRail() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const expanded = useUiStore((state) => state.sidebarExpanded);
  const showMailView = useUiStore((state) => state.showMailView);
  const showSettings = useUiStore((state) => state.showSettings);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const dragRegion = useWindowDragRegion();
  const { accounts, account: selected } = useSelectedAccount();
  const foldersId = useId();
  const accountList = accounts.data ?? [];
  const mailboxes = useMailboxes(selected?.id).data ?? [];
  /** Rail views that take dragged rows: they move into the folder the view shows. */
  const dropFolder = (id: (typeof MAIL_VIEW_IDS)[number]) =>
    id === "inbox" || id === "archive" || id === "trash" ? mailboxWithRole(mailboxes, id) : undefined;
  // Stage 8: Cmd/Ctrl+1 to 8 open the views (as the user assigned them).
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const shortcutsOf = useBoundShortcuts(platform);
  const viewShortcut = (id: (typeof MAIL_VIEW_IDS)[number]) => {
    const command = (Object.keys(VIEW_COMMANDS) as ViewCommandId[]).find((key) => VIEW_COMMANDS[key] === id);
    const first = command ? shortcutsOf(command)[0] : undefined;
    return first && { hint: formatShortcut(first, platform), aria: ariaShortcut(first, platform) };
  };

  return (
    <nav
      aria-label={t("nav.label")}
      className={cx(
        // Decision 0031: the darkest tonal step of the frame, a hairline to the list.
        "flex h-full shrink-0 flex-col border-r border-border-subtle bg-rail",
        expanded ? "w-(--ui-sidebar-width)" : "w-(--ui-rail-width) items-center",
      )}
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
          "flex min-h-0 w-full flex-1 flex-col overflow-x-hidden overflow-y-auto py-3",
          "[mask-image:linear-gradient(to_bottom,transparent,black_var(--ui-space-2),black_calc(100%-var(--ui-space-2)),transparent)]",
          expanded ? "gap-4 px-3 [scrollbar-width:thin]" : "items-center [scrollbar-width:none]",
        )}
      >
        <ul className={cx("flex w-full flex-col gap-1", !expanded && "items-center")}>
          {MAIL_VIEW_IDS.map((id) => (
            <li key={id} className={cx(expanded && "w-full")}>
              {id === "folders" ? (
                <RailButton
                  icon={MAIL_VIEW_ICONS[id]}
                  label={t(`nav.${id}`)}
                  expanded={expanded}
                  disclosure={{ expanded, ...(expanded && { controls: foldersId }) }}
                  onClick={toggleSidebar}
                  data-testid={`rail-${id}`}
                />
              ) : (
                <RailButton
                  icon={MAIL_VIEW_ICONS[id]}
                  label={t(`nav.${id}`)}
                  expanded={expanded}
                  active={view.kind === "mail" && view.mailView === id}
                  onClick={() => {
                    showMailView(id);
                  }}
                  dropMailbox={dropFolder(id)}
                  shortcut={viewShortcut(id)}
                  data-testid={`rail-${id}`}
                />
              )}
            </li>
          ))}
        </ul>
        {expanded && (
          <div id={foldersId} className="flex flex-col gap-4 border-t border-border-subtle pt-4">
            {/* Stage 16: favourite folders of every account, at the top. */}
            <FavoriteFolders accounts={accountList} />
            <UnifiedTree accountCount={accountList.length} />
            {/* Stage 8: no empty band without accounts; the view next to it offers to add one. */}
            {accounts.isSuccess && accountList.length === 0 && (
              <p className="px-2 text-12 text-tertiary" data-testid="sidebar-no-accounts">
                {t("presentation.sidebar.noAccounts")}
              </p>
            )}
            {/* Stage 7: saved searches as views. */}
            <SavedSearchNav />
            {accountList.map((account) => (
              <AccountFolders key={account.id} account={account} selected={account.id === selected?.id} />
            ))}
          </div>
        )}
      </div>
      {/* A hairline above the bottom group: where the views scroll under it in a low window. */}
      <div className={cx("flex shrink-0 flex-col gap-2 border-t border-border-subtle py-3", expanded ? "px-3" : "items-center")}>
        {/* Account switcher: one entry per account with its colour and name. Empty without accounts. */}
        {expanded ? null : <AccountSwitcher accounts={accountList} selected={selected} />}
        <ActivityButton accounts={accountList} expanded={expanded} active={view.kind === "activity"} />
        <OutboxButton accounts={accountList} expanded={expanded} active={view.kind === "outbox"} />
        <RailButton
          icon={Settings}
          label={t("nav.settings")}
          expanded={expanded}
          active={view.kind === "settings"}
          onClick={() => {
            showSettings();
          }}
          data-testid="rail-settings"
        />
      </div>
    </nav>
  );
}
