import { EyeOff, PanelLeftClose, SlidersHorizontal } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { ResizeHandle } from "../components/ResizeHandle";
import { Tooltip } from "../components/Tooltip";
import { treeItemAria, useTreeNavigation } from "../components/useTreeNavigation";
import { useAccountDropTarget } from "../features/accounts/accountDrag";
import { useAccountArrange } from "../features/accounts/arrange";
import { useSelectedAccount } from "../features/accounts/selection";
import { OverflowMenu, type OverflowItem } from "../features/actions/OverflowMenu";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "../features/actions/shortcuts";
import { useAppInfo } from "../features/app-info/queries";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { useColumnViews, useColumnViewsStore, useShownColumnViews, type ColumnViewId } from "../features/layout/columnViews";
import { SECTION_KEY, SectionRow, useSection } from "../features/layout/SectionRow";
import { sidebarIconClass, sidebarRowClass } from "../features/layout/sidebarStyles";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import { MailboxTree } from "../features/mailboxes/MailboxTree";
import { useDragAutoScroll } from "../features/mailboxes/useDragAutoScroll";
import { useMailboxes } from "../features/mailboxes/queries";
import { isRoleView, mailboxWithRole } from "../features/mailboxes/tree";
import { FavoriteFolders, RowMenuSlot } from "../features/organize/folders";
import { SavedSearchNav } from "../features/search/SavedSearchNav";
import { UnifiedTree } from "../features/unified/UnifiedTree";
import type { Account, MailboxId } from "../services/ipc";
import { useBoundShortcuts } from "../stores/shortcuts";
import { clampSidebarWidth, SIDEBAR_WIDTH, useUiStore } from "../stores/ui";
import { StatusGroup } from "./IconRail";

/** A view row's "…": hide it from the column and the rail, or "Edit views…". */
function ViewRowMenu({ id, onHide, returnFocus }: { id: ColumnViewId; onHide: () => void; returnFocus: () => void }) {
  const { t } = useTranslation();
  const openEditor = useColumnViewsStore((state) => state.openEditor);
  const items: OverflowItem[] = [
    { id: "hide", label: t("columnViews.hide"), icon: EyeOff, onSelect: onHide, "data-testid": `views-row-hide-${id}` },
    { id: "edit", label: t("columnViews.edit"), icon: SlidersHorizontal, onSelect: openEditor, "data-testid": `views-row-edit-${id}` },
  ];
  return (
    <OverflowMenu
      label={t("columnViews.rowMenu", { name: t(`nav.${id}`) })}
      items={items}
      tabIndex={-1}
      returnFocus={returnFocus}
      narrow
      data-testid={`views-row-menu-${id}`}
    />
  );
}

/**
 * "Views": the views across folders, one tree (one Tab stop) under its label, which folds it. The
 * folders with a role (inbox, sent, ...) are not repeated here: each account's tree has them.
 * Stage 17: only the views the user chose, in the user's order ("Edit views…" on the label's
 * "…", on each view's "…" and in the palette); none chosen, a quiet hint instead of an empty band.
 */
function ViewsTree() {
  const { t } = useTranslation();
  const view = useUiStore((state) => state.view);
  const showMailView = useUiStore((state) => state.showMailView);
  const views = useColumnViews();
  const shown = useShownColumnViews();
  const choose = useColumnViewsStore((state) => state.choose);
  const openEditor = useColumnViewsStore((state) => state.openEditor);
  const [hidden, setHidden] = useState<string | null>(null);
  const headingId = useId();
  const section = useSection("views", t("nav.viewsSection"));
  const currentId = view.kind === "mail" && !(view.mailView === "savedViews" && view.savedSearchId) ? view.mailView : null;
  const items = section.open
    ? shown.map((id, index) => ({ key: id, level: 2, posinset: index + 1, setsize: shown.length, text: t(`nav.${id}`) }))
    : [];
  const nav = useTreeNavigation({
    items: [section.item, ...items],
    currentKey: currentId,
    onOpen: (key) => {
      if (key === SECTION_KEY) section.toggle();
      const id = shown.find((candidate) => candidate === key);
      if (id) showMailView(id);
    },
    onExpand: section.onExpand,
  });
  const hide = (id: ColumnViewId) => {
    const index = shown.indexOf(id);
    // The focus goes to the view after it, else the one before, else the label.
    const next = shown[index + 1] ?? shown[index - 1] ?? SECTION_KEY;
    flushSync(() => {
      choose(views.map((entry) => (entry.id === id ? { ...entry, shown: false } : entry)));
    });
    nav.focus(next);
    setHidden(t("columnViews.hidden", { name: t(`nav.${id}`) }));
  };
  const labelMenu: OverflowItem[] = [
    { id: "edit", label: t("columnViews.edit"), icon: SlidersHorizontal, onSelect: openEditor, "data-testid": "sidebar-views-edit" },
  ];
  return (
    <section aria-labelledby={headingId} className="flex flex-col">
      <h2 id={headingId} className="sr-only">
        {t("nav.viewsSection")}
      </h2>
      <ul {...nav.treeProps} aria-labelledby={headingId} className="flex flex-col" data-testid="sidebar-views">
        <SectionRow
          section={section}
          nav={nav}
          menu={
            <OverflowMenu
              label={t("columnViews.menu")}
              items={labelMenu}
              tabIndex={-1}
              returnFocus={() => {
                nav.focus(SECTION_KEY);
              }}
              narrow
              small
              data-testid="sidebar-views-menu"
            />
          }
          data-testid="sidebar-section-views"
        />
        {items.map((item) => {
          const id = item.key;
          const Icon = MAIL_VIEW_ICONS[id];
          const current = currentId === id;
          return (
            <li
              key={id}
              {...treeItemAria(item)}
              {...nav.itemProps(id)}
              aria-current={current ? "page" : undefined}
              // The view's name, not the name of its "…" button too.
              aria-label={item.text}
              onClick={() => {
                showMailView(id);
              }}
              className={sidebarRowClass(current, "group/folder gap-2 ps-3 pe-2")}
              data-testid={`sidebar-view-${id}`}
            >
              <Icon aria-hidden className={sidebarIconClass(current)} strokeWidth={1.75} />
              <span className="truncate">{item.text}</span>
              <RowMenuSlot>
                <ViewRowMenu
                  id={id}
                  onHide={() => {
                    hide(id);
                  }}
                  returnFocus={() => {
                    nav.focus(id);
                  }}
                />
              </RowMenuSlot>
            </li>
          );
        })}
      </ul>
      {/* Stage 17: every view hidden: a quiet hint, not an empty band. */}
      {section.open && shown.length === 0 && (
        <div className="flex flex-col items-start gap-1 ps-3 pe-2 pt-1" data-testid="sidebar-views-empty">
          <p className="text-12 text-tertiary">{t("columnViews.empty")}</p>
          <button
            type="button"
            onClick={openEditor}
            data-testid="sidebar-views-empty-edit"
            className="-ms-1 rounded-control px-1 text-12 font-medium text-accent transition-colors hover:bg-rail-hover"
          >
            {t("columnViews.edit")}
          </button>
        </div>
      )}
      <span aria-live="polite" className="sr-only">
        {hidden}
      </span>
    </section>
  );
}

/** The folder tree of one account, with the folder the view shows marked as current. */
function AccountFolders({ account, accounts, selected }: { account: Account; accounts: readonly Account[]; selected: boolean }) {
  const { t } = useTranslation();
  const arrange = useAccountArrange();
  const [moved, setMoved] = useState("");
  // Stage 22 (decision 0078): a dragged account heading goes before or after this account.
  const drop = useAccountDropTarget(account, accounts, (order, movedId) => {
    const dragged = accounts.find((a) => a.id === movedId);
    void arrange.reorder(order).then((ok) => {
      if (ok && dragged) {
        setMoved(t("accountEdit.moved", { name: dragged.displayName || dragged.email, position: order.indexOf(dragged.id) + 1, count: order.length }));
      }
    });
  });
  const view = useUiStore((state) => state.view);
  const unified = useUiStore((state) => state.unifiedInbox !== null);
  const mailboxes = useMailboxes(account.id);
  let current: MailboxId | null = null;
  if (view.kind === "mailbox" && view.accountId === account.id) current = view.mailboxId;
  // Stage 7: while the unified inbox is shown, no account's own inbox is the current folder.
  if (view.kind === "mail" && selected && isRoleView(view.mailView) && !(unified && view.mailView === "inbox")) {
    current = mailboxWithRole(mailboxes.data ?? [], view.mailView)?.id ?? null;
  }
  return (
    <section
      aria-label={account.displayName || account.email}
      {...drop.handlers}
      data-account-drop={drop.position ?? undefined}
      className={cx(
        "relative flex flex-col",
        // The place it goes: a line in the gap above or below the account.
        drop.position === "before" && "before:absolute before:inset-x-1 before:-top-2 before:h-1 before:rounded-full before:bg-accent-fill",
        drop.position === "after" && "after:absolute after:inset-x-1 after:-bottom-2 after:h-1 after:rounded-full after:bg-accent-fill",
      )}
      data-testid={`sidebar-account-section-${account.id}`}
    >
      <MailboxTree account={account} currentMailboxId={current} />
      <span aria-live="polite" className="sr-only">
        {moved}
      </span>
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
 * The accounts-and-folders column (docs/DESIGN.md, "Seitenspalte"), open by default: the unified
 * inbox, the views across folders, saved searches, favourites, then every account with its
 * folders, and at the bottom activity, outbox and settings. It replaces the icon rail while it
 * is open. Its width (200 to 320 px) follows the drag handle at its edge. In a window too narrow
 * for it next to the list and the reading pane it folds to the rail and opens over the content
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
        <div className="flex shrink-0 flex-col gap-5 overflow-x-clip px-2">
        <UnifiedTree accounts={accountList} />
        <ViewsTree />
        {/* Stage 7: saved searches, right below the views. */}
        <SavedSearchNav />
        {/* Stage 16: favourite folders of every account. */}
        <FavoriteFolders accounts={accountList} />
        <section aria-labelledby={accountsHeading} className="flex flex-col gap-3">
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
            <AccountFolders key={account.id} account={account} accounts={accountList} selected={account.id === selected?.id} />
          ))}
        </section>
        </div>
      </div>
      <StatusGroup accounts={accountList} expanded />
    </nav>
  );
}
