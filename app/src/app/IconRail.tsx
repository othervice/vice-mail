import { CircleExclamationPoint, CloudOff, Settings, type LucideIcon } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { accountFill } from "../features/accounts/accountColor";
import { Avatar } from "../features/accounts/Avatar";
import { useSelectedAccount } from "../features/accounts/selection";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { MailboxTree } from "../features/mailboxes/MailboxTree";
import { useMailboxes } from "../features/mailboxes/queries";
import { isRoleView, mailboxWithRole } from "../features/mailboxes/tree";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import type { Account, MailboxId } from "../services/ipc";
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
  /** Stable hook for the desktop tests (labels change with the language). */
  "data-testid": string;
}

function RailButton({ icon: Icon, label, expanded, active = false, onClick, disclosure, ...rest }: RailButtonProps) {
  // Active: accent icon on a soft fill plus a bar at the rail edge, so it is not colour alone.
  const className = cx(
    "relative flex items-center rounded-card transition-colors",
    expanded ? "h-8 w-full gap-3 px-3 text-13" : "size-(--ui-rail-item) justify-center",
    active
      ? "bg-accent-soft font-medium text-accent before:absolute before:inset-y-2 before:-left-2 before:w-1 before:rounded-control before:bg-accent"
      : "text-secondary hover:bg-hover hover:text-primary",
  );
  const button = (
    <button
      type="button"
      aria-label={label}
      aria-current={active ? "page" : undefined}
      aria-expanded={disclosure?.expanded}
      aria-controls={disclosure?.controls}
      onClick={onClick}
      className={className}
      data-testid={rest["data-testid"]}
    >
      <Icon aria-hidden className="size-(--ui-icon) shrink-0" strokeWidth={1.75} />
      {expanded && (
        <span aria-hidden className="truncate">
          {label}
        </span>
      )}
    </button>
  );
  // Same element in both modes: unfolding the sidebar keeps the focus on the button.
  return (
    <Tooltip label={label} side="right" disabled={expanded}>
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
                  "relative flex size-(--ui-rail-item) items-center justify-center rounded-card transition-colors",
                  current && accounts.length > 1 ? "bg-selected" : "hover:bg-hover",
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
  if (view.kind === "mail" && selected && isRoleView(view.mailView)) {
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
          selected ? "text-primary" : "text-secondary hover:bg-hover hover:text-primary",
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

  return (
    <nav
      aria-label={t("nav.label")}
      className={cx(
        "flex h-full shrink-0 flex-col border-r border-border bg-rail",
        expanded ? "w-(--ui-sidebar-width)" : "w-(--ui-rail-width) items-center",
      )}
    >
      {/* Room for the window buttons that macOS draws over this corner. */}
      <div aria-hidden data-tauri-drag-region={dragRegion ? "deep" : undefined} className="h-(--ui-titlebar-inset) w-full shrink-0" />
      <div className={cx("flex min-h-0 w-full flex-1 flex-col overflow-y-auto py-3", expanded ? "gap-4 px-3" : "items-center")}>
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
                  data-testid={`rail-${id}`}
                />
              )}
            </li>
          ))}
        </ul>
        {expanded && (
          <div id={foldersId} className="flex flex-col gap-4 border-t border-border pt-4">
            {accountList.map((account) => (
              <AccountFolders key={account.id} account={account} selected={account.id === selected?.id} />
            ))}
          </div>
        )}
      </div>
      <div className={cx("flex shrink-0 flex-col gap-2 py-3", expanded ? "px-3" : "items-center")}>
        {/* Account switcher: one entry per account with its colour and name. Empty without accounts. */}
        {expanded ? null : <AccountSwitcher accounts={accountList} selected={selected} />}
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
