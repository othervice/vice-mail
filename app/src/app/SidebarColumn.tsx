import { PanelLeftClose } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, type KeyboardEvent, type RefObject } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { ResizeHandle } from "../components/ResizeHandle";
import { Tooltip } from "../components/Tooltip";
import { useAccountMoveByDrag } from "../features/accounts/accountDrag";
import { useSelectedAccount } from "../features/accounts/selection";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "../features/actions/shortcuts";
import { useAppInfo } from "../features/app-info/queries";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { useSidebarFoldStore } from "../features/layout/sidebarFolds";
import { MailboxTree } from "../features/mailboxes/MailboxTree";
import { useDragAutoScroll } from "../features/mailboxes/useDragAutoScroll";
import { useMailboxes } from "../features/mailboxes/queries";
import { isRoleView, mailboxWithRole } from "../features/mailboxes/tree";
import { FavoriteFolders } from "../features/organize/folders";
import type { Account, MailboxId } from "../services/ipc";
import { useBoundShortcuts } from "../stores/shortcuts";
import { clampSidebarWidth, SIDEBAR_WIDTH, useUiStore } from "../stores/ui";
import { AllAccountsTree } from "./AllAccountsTree";
import { SidebarFooter } from "./SidebarFooter";

interface AccountFoldersProps {
  account: Account;
  selected: boolean;
  /** Stage 23 (decision 0085): "All accounts" has its own Sent (several accounts). */
  sentAbove: boolean;
  /** Stage 23: the line of a dragged account in the gap before or after this one. */
  line: "before" | "after" | null;
  /** This account is the one being dragged. */
  dimmed: boolean;
  /** Marks it as an entry of the list that takes dragged accounts. */
  dropItem: Record<string, string>;
}

/** The folder tree of one account, with the folder the view shows marked as current. */
function AccountFolders({ account, selected, sentAbove, line, dimmed, dropItem }: AccountFoldersProps) {
  const view = useUiStore((state) => state.view);
  const unified = useUiStore((state) => state.unifiedInbox !== null);
  const mailboxes = useMailboxes(account.id);
  let current: MailboxId | null = null;
  if (view.kind === "mailbox" && view.accountId === account.id) current = view.mailboxId;
  // Stage 7: while the unified inbox is shown, no account's own inbox is the current folder.
  // Stage 23: Sent of "All accounts" is the current row of the Sent view, not the account's.
  if (
    view.kind === "mail" &&
    selected &&
    isRoleView(view.mailView) &&
    !(unified && view.mailView === "inbox") &&
    !(sentAbove && view.mailView === "sent")
  ) {
    current = mailboxWithRole(mailboxes.data ?? [], view.mailView)?.id ?? null;
  }
  return (
    <section
      aria-label={account.displayName || account.email}
      {...dropItem}
      data-account-drop={line ?? undefined}
      data-account-dragged={dimmed || undefined}
      className={cx(
        "relative flex flex-col",
        // The account being dragged stays in its place, dimmed, until it is let go.
        dimmed && "opacity-50",
        // The place it goes: a line in the middle of the 8 px gap above or below the account.
        line === "before" &&
          "before:absolute before:inset-x-1 before:-top-[calc(var(--ui-space-1)*1.5)] before:h-1 before:rounded-full before:bg-accent-fill",
        line === "after" &&
          "after:absolute after:inset-x-1 after:-bottom-[calc(var(--ui-space-1)*1.5)] after:h-1 after:rounded-full after:bg-accent-fill",
      )}
      data-testid={`sidebar-account-section-${account.id}`}
    >
      <MailboxTree account={account} currentMailboxId={current} sentAsFolder={sentAbove} />
    </section>
  );
}

/** The drag handle at the column's edge (WAI-ARIA window splitter): arrows, Home, End, Enter. */
function SidebarResizeHandle({ controls }: { controls: string }) {
  const { t } = useTranslation();
  const width = useUiStore((state) => state.sidebarWidth);
  const setWidth = useUiStore((state) => state.setSidebarWidth);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  return (
    <ResizeHandle
      orientation="vertical"
      value={width}
      limits={SIDEBAR_WIDTH}
      onChange={setWidth}
      // The splitter's own key: folds the column (its toggle in the header does the same).
      onEnter={toggleSidebar}
      label={t("nav.resizeSidebar")}
      valueText={t("nav.sidebarWidth", { width })}
      controls={controls}
      className="absolute inset-y-0 -end-1"
      data-testid="sidebar-resize"
    />
  );
}

/** Closes the column that lies over the content: Escape, a click or the focus outside, a view opened. */
function useOverlayDismiss(column: RefObject<HTMLElement | null>, overlay: boolean) {
  const close = useUiStore((state) => state.closeSidebarOverlay);
  useEffect(() => {
    if (!overlay) return undefined;
    const node = column.current;
    // Opened with the button, shortcut or palette: the focus goes to the entry shown right now.
    const target =
      node?.querySelector<HTMLElement>('[role="treeitem"][aria-current="page"]') ?? node?.querySelector<HTMLElement>('[role="treeitem"][tabindex="0"]');
    target?.focus();
    // Sections still loading (favourites, saved searches) push the row down: keep it in view
    // while the column fills, for a moment after opening.
    const keepInView = new MutationObserver(() => {
      if (target && document.activeElement === target) target.scrollIntoView?.({ block: "nearest" });
    });
    if (node) keepInView.observe(node, { childList: true, subtree: true });
    const settle = window.setTimeout(() => {
      keepInView.disconnect();
    }, 1000);
    const inside = (element: EventTarget | null) => element instanceof Node && (node?.contains(element) ?? false);
    const onPointerDown = (event: globalThis.PointerEvent) => {
      if (!inside(event.target)) close();
    };
    const onFocusIn = (event: FocusEvent) => {
      if (!inside(event.target)) close();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("focusin", onFocusIn);
    const offView = useUiStore.subscribe((state, previous) => {
      if (state.view === previous.view) return;
      const hadFocus = inside(document.activeElement);
      close();
      // What had the focus is gone with the column: the main area takes it (stage 8).
      if (hadFocus) document.querySelector<HTMLElement>("main")?.focus();
    });
    return () => {
      keepInView.disconnect();
      window.clearTimeout(settle);
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("focusin", onFocusIn);
      offView();
    };
  }, [overlay, close, column]);
  return (event: KeyboardEvent<HTMLElement>) => {
    const fromInside = event.target instanceof Node && (column.current?.contains(event.target) ?? false);
    if (!overlay || event.key !== "Escape" || event.defaultPrevented || !fromInside) return;
    event.preventDefault();
    // Rendered at once: the rail is inert while the column lies over it, and takes no focus.
    flushSync(close);
    // Back to the rail's "Folders", which opened it.
    document.querySelector<HTMLElement>('[data-testid="rail-folders"]')?.focus();
  };
}

/**
 * The accounts-and-folders column (docs/DESIGN.md, "Seitenspalte"), open by default. Stage 23
 * (decision 0085), the approved draft: "All accounts" (the unified inbox, Later, Flagged, Sent and
 * "More" with every other view, the groups, saved searches, Activity and the outbox), the
 * favourites, one section per account (its inbox and Sent, "More folders"), and at the foot when
 * the accounts last synced, "Sync now" and Settings. It replaces the icon rail while it is open.
 * Its width (200 to 320 px) follows the drag handle at its edge. In a window too narrow for it
 * next to the list and the reading pane it folds to the rail and opens over the content
 * (`overlay`) instead.
 */
export function SidebarColumn({ overlay }: { overlay: boolean }) {
  const { t } = useTranslation();
  const width = useUiStore((state) => state.sidebarWidth);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const dragRegion = useWindowDragRegion();
  const { accounts, account: selected } = useSelectedAccount();
  const accountList = accounts.data ?? [];
  const column = useRef<HTMLElement>(null);
  const contentId = useId();
  const accountsHeading = useId();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const shortcut = useBoundShortcuts(platform)("toggleSidebar")[0];
  const label = t("nav.hideSidebar");
  const onKeyDown = useOverlayDismiss(column, overlay);
  const autoScroll = useDragAutoScroll();
  const accountDrop = useAccountMoveByDrag(accountList);
  const movedAccount = accountDrop.announcement;
  // Stage 23 (decision 0085): what is opened or closed of a removed account is dropped on the next save.
  const setFoldAccounts = useSidebarFoldStore((state) => state.setAccounts);
  const loaded = accounts.isSuccess;
  const accountIds = accountList.map((account) => account.id).join(",");
  useEffect(() => {
    if (loaded) setFoldAccounts(accountIds === "" ? [] : accountIds.split(","));
  }, [loaded, accountIds, setFoldAccounts]);
  // "Go to Folders" (palette): the entry shown right now takes the focus, else the first tree's.
  const focusRequested = useUiStore((state) => state.sidebarFocusRequested);
  const sidebarFocused = useUiStore((state) => state.sidebarFocused);
  useEffect(() => {
    if (!focusRequested) return;
    sidebarFocused();
    const node = column.current;
    const target =
      node?.querySelector<HTMLElement>('[role="treeitem"][aria-current="page"]') ?? node?.querySelector<HTMLElement>('[role="treeitem"][tabindex="0"]');
    target?.focus();
  }, [focusRequested, sidebarFocused]);

  // The width as a variable on the column: CSSOM, not a style attribute (CSP style-src 'self').
  useLayoutEffect(() => {
    column.current?.style.setProperty("--ui-sidebar-width", `${String(clampSidebarWidth(width) / 16)}rem`);
  }, [width]);

  return (
    <nav
      ref={column}
      aria-label={t("nav.sidebar")}
      onKeyDown={onKeyDown}
      className={cx(
        // Decision 0031: the darkest tonal step of the frame, a hairline to the list.
        "flex h-full w-(--ui-sidebar-width) shrink-0 flex-col border-r border-border-subtle bg-rail",
        overlay ? "absolute inset-y-0 start-0 z-30 shadow-panel" : "relative",
      )}
      data-testid="sidebar-column"
      data-overlay={overlay || undefined}
      // Decision 0079: docked, the column is chrome (Mica shows through it on Windows 11); lying
      // over the content it stays opaque.
      data-chrome={overlay ? undefined : true}
    >
      {/* As high as the list header; on macOS the window buttons sit at its start. */}
      <div data-tauri-drag-region={dragRegion ? true : undefined} className="flex h-(--ui-header-height) shrink-0 items-center justify-end px-2">
        <Tooltip label={shortcut ? t("actions.withShortcut", { label, shortcut: formatShortcut(shortcut, platform) }) : label} side="right">
          <button
            type="button"
            aria-label={label}
            aria-expanded
            aria-controls={contentId}
            aria-keyshortcuts={shortcut && ariaShortcut(shortcut, platform)}
            onClick={toggleSidebar}
            // The same test id as the rail's "Folders": the one control that folds and unfolds the column.
            data-testid="rail-folders"
            className="flex size-8 items-center justify-center rounded-control text-secondary transition-colors hover:bg-rail-hover hover:text-primary"
          >
            <PanelLeftClose aria-hidden className="size-(--ui-icon)" strokeWidth={1.75} />
          </button>
        </Tooltip>
      </div>
      {!overlay && <SidebarResizeHandle controls={contentId} />}
      {/* Scrolls on its own, never sideways; the edges fade so a cut-off row reads as "more".
          Stage 21: what is inside clips sideways (`overflow-x: clip` on the inner block, which
          does not scroll), so nothing wider, a menu included, can make the column scroll
          sideways; should anything still do it, it goes straight back. */}
      <div
        id={contentId}
        onScroll={(event) => {
          if (event.currentTarget.scrollLeft !== 0) event.currentTarget.scrollLeft = 0;
        }}
        // Stage 22 (decision 0081): dragged rows or an account scroll the column at its edges.
        onDragOver={autoScroll}
        className={cx(
          "flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto pt-1 pb-4 [scrollbar-width:thin]",
          "[mask-image:linear-gradient(to_bottom,transparent,black_var(--ui-space-2),black_calc(100%-var(--ui-space-2)),transparent)]",
        )}
        data-testid="sidebar-scroll"
      >
        {/* Stage 23 (decision 0085): 8 px between the sections, as in the draft. */}
        <div className="flex shrink-0 flex-col gap-2 overflow-x-clip px-2">
          <AllAccountsTree accounts={accountList} />
          {/* Stage 16: favourite folders of every account. */}
          <FavoriteFolders accounts={accountList} />
          {/* Stage 23: a dragged account heading goes where the line shows (decision 0078). */}
          <section aria-labelledby={accountsHeading} {...accountDrop.zoneProps} className="flex flex-col gap-2" data-testid="sidebar-accounts">
            <h2 id={accountsHeading} className="sr-only">
              {t("nav.accounts")}
            </h2>
            {/* Stage 8: no empty band without accounts; the view next to it offers to add one. */}
            {accounts.isSuccess && accountList.length === 0 && (
              <p className="px-2 text-12 text-tertiary" data-testid="sidebar-no-accounts">
                {t("presentation.sidebar.noAccounts")}
              </p>
            )}
            {accountList.map((account) => (
              <AccountFolders
                key={account.id}
                account={account}
                selected={account.id === selected?.id}
                sentAbove={accountList.length > 1}
                line={accountDrop.lineOf(account.id)}
                dimmed={accountDrop.dragged === account.id}
                dropItem={accountDrop.itemProps(account.id)}
              />
            ))}
            <span aria-live="polite" className="sr-only">
              {movedAccount}
            </span>
          </section>
        </div>
      </div>
      <SidebarFooter accounts={accountList} />
    </nav>
  );
}
