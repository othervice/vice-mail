import { CircleExclamationPoint, CloudOff, RefreshCw, Settings, type LucideIcon } from "lucide-react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { useRovingFocus } from "../components/useRovingFocus";
import { allOffline } from "../features/accounts/selection";
import { ipc, toCoreError, type Account } from "../services/ipc";
import { useNoticeStore } from "../stores/notices";
import { useUiStore } from "../stores/ui";

/** What the foot says about syncing, and how loud. */
export interface SyncSummary {
  text: string;
  /** The longer form (tooltip): which accounts and why. */
  detail: string;
  tone: "quiet" | "offline" | "error";
}

function startOfDay(ms: number): number {
  const day = new Date(ms);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
}

/**
 * Stage 23 (decision 0085): the foot's line from the accounts' sync state, with the status texts of
 * the list header. An account that does not sync, else one that is offline, comes first (the
 * whole app offline: "Offline"); while one syncs "Syncing…"; else "Synced at 18:04", the time
 * the last of the accounts finished a pass (with the date if not today). Null without accounts or
 * before the first pass.
 */
export function syncSummary(accounts: readonly Account[], t: TFunction, language: string, now = Date.now()): SyncSummary | null {
  if (accounts.length === 0) return null;
  const name = (account: Account) => account.displayName || account.email;
  const names = (list: readonly Account[]) => list.map(name).join(", ");
  const trouble = (list: readonly Account[], state: string, tone: SyncSummary["tone"]): SyncSummary => {
    const [first] = list;
    const text = list.length === 1 && first ? t("sidebar.footer.account", { name: name(first), state }) : t("sidebar.footer.accounts", { count: list.length, state });
    return { text, detail: `${state}: ${names(list)}`, tone };
  };
  const failed = accounts.filter((account) => account.status.state === "error");
  if (failed.length > 0) return trouble(failed, t("status.error"), "error");
  if (allOffline(accounts)) return { text: t("status.offline"), detail: t("status.allOffline"), tone: "offline" };
  const offline = accounts.filter((account) => account.status.state === "offline");
  if (offline.length > 0) return trouble(offline, t("status.offline"), "offline");
  if (accounts.some((account) => account.status.state === "connecting" || account.status.state === "syncing")) {
    return { text: t("status.syncing"), detail: t("status.syncing"), tone: "quiet" };
  }
  const passes = accounts.map((account) => account.status.lastSyncAt).filter((at): at is number => at !== null);
  if (passes.length === 0) return null;
  const at = Math.min(...passes);
  const time = new Intl.DateTimeFormat(language, { timeStyle: "short" }).format(at);
  const text =
    startOfDay(at) === startOfDay(now)
      ? t("sidebar.footer.syncedAt", { time })
      : t("sidebar.footer.syncedOn", { date: new Intl.DateTimeFormat(language, { day: "numeric", month: "numeric" }).format(at), time });
  return { text, detail: text, tone: "quiet" };
}

const TONE_ICON: Record<SyncSummary["tone"], LucideIcon | null> = { quiet: null, offline: CloudOff, error: CircleExclamationPoint };

function FootButton({ icon: Icon, label, active = false, onClick, roving, ...rest }: {
  icon: LucideIcon;
  label: string;
  active?: boolean;
  onClick: () => void;
  roving: ReturnType<ReturnType<typeof useRovingFocus>["itemProps"]>;
  "data-testid": string;
}) {
  return (
    <Tooltip label={label} side="top">
      <button
        type="button"
        aria-label={label}
        aria-current={active ? "page" : undefined}
        onClick={onClick}
        {...roving}
        data-testid={rest["data-testid"]}
        className={cx(
          "flex size-6 shrink-0 items-center justify-center rounded-control transition-colors",
          active ? "bg-rail-selected text-accent" : "text-tertiary hover:bg-rail-hover hover:text-primary",
        )}
      >
        <Icon aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.9} />
      </button>
    </Tooltip>
  );
}

const SYNC_KEY = "sync";
const SETTINGS_KEY = "settings";

/**
 * The foot of the column (stage 23, decision 0085, the approved draft): when the accounts last
 * synced, or which one is offline or stopped; "Sync now" (a full pass of every account, a
 * reconnect for one that is offline); Settings. The two buttons are one Tab stop.
 */
export function SidebarFooter({ accounts }: { accounts: readonly Account[] }) {
  const { t, i18n } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showSettings = useUiStore((state) => state.showSettings);
  const push = useNoticeStore((state) => state.push);
  const summary = syncSummary(accounts, t, i18n.language);
  const toolbar = useRovingFocus({ keys: accounts.length > 0 ? [SYNC_KEY, SETTINGS_KEY] : [SETTINGS_KEY], activeKey: view.kind === "settings" ? SETTINGS_KEY : null, orientation: "horizontal" });
  const syncNow = () => {
    for (const account of accounts) {
      ipc.reloadFolders(account.id).catch((error: unknown) => {
        push({ kind: "error", context: "sync", error: toCoreError(error) });
      });
    }
  };
  const ToneIcon = summary ? TONE_ICON[summary.tone] : null;
  return (
    // Its hairline stops 8 px before the column's edges, as in the draft.
    <div className="mx-2 flex shrink-0 items-center gap-2 border-t border-border-subtle py-2" data-testid="sidebar-footer">
      <p
        title={summary?.detail}
        data-testid="sidebar-sync-status"
        data-tone={summary?.tone}
        className={cx(
          "flex min-w-0 flex-1 items-center gap-1 ps-2 text-11 tabular-nums",
          summary?.tone === "error" ? "text-danger" : summary?.tone === "offline" ? "text-secondary" : "text-tertiary",
        )}
      >
        {ToneIcon && <ToneIcon aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />}
        <span className="truncate">{summary?.text}</span>
      </p>
      <div role="toolbar" aria-label={t("sidebar.footer.label")} {...toolbar.groupProps} className="flex shrink-0 items-center gap-1">
        {accounts.length > 0 && (
          <FootButton icon={RefreshCw} label={t("sidebar.footer.syncNow")} onClick={syncNow} roving={toolbar.itemProps(SYNC_KEY)} data-testid="sidebar-sync-now" />
        )}
        <FootButton
          icon={Settings}
          label={t("nav.settings")}
          active={view.kind === "settings"}
          onClick={() => {
            showSettings();
          }}
          roving={toolbar.itemProps(SETTINGS_KEY)}
          data-testid="rail-settings"
        />
      </div>
    </div>
  );
}
