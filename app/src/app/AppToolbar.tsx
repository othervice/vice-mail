import { PanelLeft, PanelRight, Search, SquarePen } from "lucide-react";
import { useLayoutEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { Tooltip } from "../components/Tooltip";
import { ToolbarButton } from "../components/ToolbarButton";
import { openComposer } from "../stores/composer";
import { useLayoutStore } from "../stores/layout";
import { useSearchStore } from "../stores/search";
import { useBoundShortcuts } from "../stores/shortcuts";
import { useToolbarStore } from "../stores/toolbar";
import { clampSidebarWidth, useUiStore, type View } from "../stores/ui";
import { useSelectedAccount } from "../features/accounts/selection";
import { ariaShortcut, formatShortcut, shortcutPlatform } from "../features/actions/shortcuts";
import { AiPanelToggle } from "../features/ai/AiPanel";
import { useAppInfo } from "../features/app-info/queries";
import { useWindowDragRegion } from "../features/app-info/useWindowDragRegion";
import { CONTACT_PANEL_ID, useContactPanelFits } from "../features/contacts/ContactPanel";

/** The accounts-and-folders column stands next to the content (not folded, the window wide enough). */
const isDocked = (state: { sidebarExpanded: boolean; sidebarNarrow: boolean }) => state.sidebarExpanded && !state.sidebarNarrow;

/** The view shows mail: search and the contact panel belong to it. */
const isMailView = (state: { view: View }) => state.view.kind === "mailbox" || (state.view.kind === "mail" && state.view.mailView !== "contacts");

/**
 * The list segment ends where the list beside the reading pane ends, so the title and the list's
 * controls stand over the list and the message's actions over the reading pane, as in the draft.
 * Without such a list (settings, the pane below the list or in its place) it is as wide as its
 * content. Measured, as the list's width follows the window, the sidebar and the handle between
 * list and pane.
 */
function useListSegment(segment: RefObject<HTMLDivElement | null>, frame: RefObject<HTMLDivElement | null>) {
  const column = useToolbarStore((state) => state.listColumn);
  useLayoutEffect(() => {
    const element = segment.current;
    if (!element) return undefined;
    if (!column) {
      element.style.removeProperty("width");
      return undefined;
    }
    const update = () => {
      const width = column.getBoundingClientRect().right - element.getBoundingClientRect().left;
      if (width > 0) element.style.setProperty("width", `${String(width)}px`);
      else element.style.removeProperty("width");
    };
    update();
    if (typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(update);
    observer.observe(column);
    if (frame.current) observer.observe(frame.current);
    window.addEventListener("resize", update);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
      element.style.removeProperty("width");
    };
  }, [column, segment, frame]);
}

/** The search of the mail view, drawn as a field: a click or its shortcut opens it above the list. */
function SearchField() {
  const { t } = useTranslation();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const shortcut = useBoundShortcuts(platform)("search")[0];
  const openSearch = useSearchStore((state) => state.openSearch);
  const open = useSearchStore((state) => state.open);
  const hint = shortcut && formatShortcut(shortcut, platform);
  return (
    <Tooltip label={hint ? t("actions.withShortcut", { label: t("search.open"), shortcut: hint }) : t("search.open")}>
      {/* Named by its text ("Search"; only visually hidden where the field shows its symbol only). */}
      <button
        type="button"
        aria-keyshortcuts={shortcut && ariaShortcut(shortcut, platform)}
        onClick={openSearch}
        // Narrow windows: its symbol only; wide ones: the draft's 260 px field.
        className={cx(
          "flex h-(--ui-toolbar-control) w-8 shrink-0 items-center justify-center gap-2 overflow-hidden rounded-control border bg-button text-13 text-tertiary transition-colors hover:text-secondary",
          "@min-[70rem]/toolbar:w-(--ui-toolbar-search-compact) @min-[70rem]/toolbar:justify-start @min-[70rem]/toolbar:px-2 @min-[90rem]/toolbar:w-(--ui-toolbar-search-width)",
          open ? "border-accent" : "border-border",
        )}
        data-testid="toolbar-search"
      >
        <Search aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
        <span className="sr-only min-w-0 flex-1 truncate text-start @min-[70rem]/toolbar:not-sr-only">{t("search.open")}</span>
        {hint && (
          <span aria-hidden className="hidden shrink-0 text-11 tabular-nums @min-[70rem]/toolbar:inline">
            {hint}
          </span>
        )}
      </button>
    </Tooltip>
  );
}

/** Shows and hides the contact panel (stored with the window's layout on this device). */
function ContactPanelToggle() {
  const { t } = useTranslation();
  const on = useLayoutStore((state) => state.contactPanel);
  const setLayout = useLayoutStore((state) => state.setLayout);
  return (
    <ToolbarButton
      icon={PanelRight}
      framed
      label={t("contactPanel.toggle")}
      hint={on ? t("contactPanel.hide") : t("contactPanel.show")}
      aria-expanded={on}
      aria-controls={on ? CONTACT_PANEL_ID : undefined}
      onClick={() => {
        setLayout({ contactPanel: !on });
      }}
      data-testid="toolbar-contact-panel"
    />
  );
}

/**
 * Stage 23, step 3 (decision 0084): the toolbar across the whole window, 52 px, as in the approved
 * draft. On macOS it is the title bar: the window buttons sit at its start (tauri.conf.json,
 * `trafficLightPosition`) and dragging it anywhere but on a control moves the window; on Windows
 * it lies below the native title bar (decision 0079) and lets Mica show through like the sidebar.
 *
 * Its segments: over the sidebar the sidebar's toggle and New mail; over the list the view's title
 * with its counter and the list's controls (`ViewHeader`, through `ToolbarPortal`); over the
 * reading pane the open message's actions (the reading pane's toolbar, through `ToolbarPortal`),
 * then the search field, the AI panel's and the contact panel's toggles.
 */
export function AppToolbar() {
  const { t } = useTranslation();
  const docked = useUiStore(isDocked);
  const overlayOpen = useUiStore((state) => state.sidebarNarrow && state.sidebarOverlay);
  const width = useUiStore((state) => state.sidebarWidth);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const mailView = useUiStore(isMailView);
  const dragRegion = useWindowDragRegion();
  const { account } = useSelectedAccount();
  const panelFits = useContactPanelFits();
  const platform = shortcutPlatform(useAppInfo().data?.platform);
  const sidebarShortcut = useBoundShortcuts(platform)("toggleSidebar")[0];
  const setSlots = useToolbarStore((state) => state.setSlots);
  const frame = useRef<HTMLDivElement>(null);
  const listSlot = useRef<HTMLDivElement>(null);
  const readingSlot = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    setSlots(listSlot.current, readingSlot.current);
    return () => {
      setSlots(null, null);
    };
  }, [setSlots]);
  // Over the docked column the segment is as wide as the column (CSSOM: CSP style-src).
  useLayoutEffect(() => {
    frame.current?.style.setProperty("--ui-sidebar-width", `${String(clampSidebarWidth(width) / 16)}rem`);
  }, [width]);
  useListSegment(listSlot, frame);

  const expanded = docked || overlayOpen;
  const sidebarLabel = expanded ? t("nav.hideSidebar") : t("nav.showSidebar");

  return (
    <header
      data-tauri-drag-region={dragRegion ? "deep" : undefined}
      // Decision 0079: chrome; on Windows 11 Mica shows through it like through the sidebar.
      data-chrome
      className="@container/toolbar flex h-(--ui-toolbar-height) shrink-0 items-center border-b border-border-subtle bg-toolbar"
      data-testid="app-toolbar"
    >
      <div
        ref={frame}
        className={cx(
          "flex h-full shrink-0 items-center justify-end gap-2 ps-[calc(var(--ui-window-buttons-end)+var(--ui-space-3))] pe-3",
          docked && "w-(--ui-sidebar-width)",
        )}
        data-testid="toolbar-frame"
      >
        <ToolbarButton
          icon={PanelLeft}
          framed
          label={sidebarLabel}
          hint={sidebarShortcut ? t("actions.withShortcut", { label: sidebarLabel, shortcut: formatShortcut(sidebarShortcut, platform) }) : sidebarLabel}
          aria-expanded={expanded}
          aria-keyshortcuts={sidebarShortcut && ariaShortcut(sidebarShortcut, platform)}
          onClick={toggleSidebar}
          // While the column is docked this is the one control that folds it (the rail's "Folders"
          // takes the id while the rail stands instead).
          data-testid={docked ? "rail-folders" : "toolbar-sidebar"}
        />
        <ToolbarButton
          icon={SquarePen}
          framed
          label={t("mail.compose")}
          disabled={!account}
          onClick={() => {
            if (account) openComposer({ kind: "new", accountId: account.id });
          }}
          data-testid="toolbar-compose"
        />
      </div>
      <div ref={listSlot} className="flex h-full min-w-0 shrink-0 items-center overflow-hidden ps-4 pe-2" data-testid="toolbar-list" />
      <div className="flex h-full min-w-0 flex-1 items-center gap-2 ps-3 pe-3">
        <div ref={readingSlot} className="flex h-full min-w-0 flex-1 items-center overflow-hidden" data-testid="toolbar-reading" />
        {mailView && <SearchField />}
        {mailView && <AiPanelToggle />}
        {mailView && panelFits && <ContactPanelToggle />}
      </div>
    </header>
  );
}
