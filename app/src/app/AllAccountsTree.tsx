import { ChevronDown, ChevronRight, EyeOff, History, Inbox, Layers, Search, Send, SendHorizontal, SlidersHorizontal, type LucideIcon } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import { cx } from "../components/cx";
import { treeItemAria, useTreeNavigation, type TreeNavItem } from "../components/useTreeNavigation";
import { OverflowMenu, type OverflowItem } from "../features/actions/OverflowMenu";
import { useColumnGroups, useColumnViews, useColumnViewsStore, type ColumnViewId } from "../features/layout/columnViews";
import { SECTION_KEY, SectionRow, useSection } from "../features/layout/SectionRow";
import { useFold } from "../features/layout/sidebarFolds";
import { indentClass, sidebarIconClass, sidebarRowClass } from "../features/layout/sidebarStyles";
import { MAIL_VIEW_ICONS } from "../features/mail/views";
import { UnreadCount } from "../features/mailboxes/MailboxTree";
import { DROP_REFUSED_CLASS, dropTargetState, useDropTarget } from "../features/mailboxes/useDropTarget";
import { RowMenuSlot } from "../features/organize/folders";
import { useSavedSearches } from "../features/search/savedQueries";
import { sameUnifiedView, UnifiedRowMenu, useUnifiedEntries, type UnifiedEntry } from "../features/unified/unifiedEntries";
import type { Account } from "../services/ipc";
import { useUiStore } from "../stores/ui";
import { RailCount, useActivityEntry, useOutboxEntry, type StatusEntry } from "./IconRail";

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

/** The count of Activity or the outbox: quiet while things wait, a red pill once they need the user. */
function StatusCount({ entry, ...rest }: { entry: StatusEntry; "data-testid": string }) {
  if (!entry.count) return null;
  return (
    <>
      {" "}
      {entry.attention > 0 ? (
        <RailCount value={entry.count} attention trailing data-testid={rest["data-testid"]} />
      ) : (
        <span aria-hidden data-testid={rest["data-testid"]} className="ms-auto shrink-0 ps-2 text-12 text-tertiary tabular-nums">
          {entry.count}
        </span>
      )}
    </>
  );
}

/** One row of the tree (a view, an inbox, a saved search, "More" …). */
interface Entry {
  item: TreeNavItem;
  icon: LucideIcon;
  /** The name a screen reader hears (starts with the visible text). */
  label: string;
  current: boolean;
  /** The row carries the fill of the entry shown: a closed "More" with the shown view inside. */
  filled?: boolean;
  /** Quieter text (the "More" row). */
  quiet?: boolean;
  count?: ReactNode;
  menu?: ReactNode;
  onOpen: () => void;
  /** Dragged rows go to the inbox of their own account (as in the rail). */
  dropInbox?: boolean;
  "data-testid": string;
}

function EntryRow({ entry, nav }: { entry: Entry; nav: ReturnType<typeof useTreeNavigation> }) {
  const { item, icon: Icon, current } = entry;
  const drop = useDropTarget(entry.dropInbox ? { kind: "role", role: "inbox" } : undefined, { id: `all-${item.key}`, folder: item.text });
  return (
    <li
      {...treeItemAria(item)}
      {...nav.itemProps(item.key)}
      aria-current={current ? "page" : undefined}
      // The row's name, not the name of its "…" button too.
      aria-label={entry.label}
      onClick={entry.onOpen}
      {...(entry.dropInbox && drop.handlers)}
      data-drop-target={entry.dropInbox ? dropTargetState(drop) : undefined}
      className={sidebarRowClass(
        current || entry.filled === true,
        cx(
          "group/folder gap-2 pe-2",
          indentClass(item.level - 1),
          entry.quiet && !current && "text-secondary",
          drop.over && "bg-accent-soft outline-2 -outline-offset-2 outline-accent",
          drop.refused && DROP_REFUSED_CLASS,
        ),
      )}
      data-testid={entry["data-testid"]}
    >
      <Icon aria-hidden className={sidebarIconClass(current)} strokeWidth={1.75} />
      <span className="truncate">{item.text}</span>
      {entry.count}
      {entry.menu && <RowMenuSlot>{entry.menu}</RowMenuSlot>}
    </li>
  );
}

/**
 * Stage 23 (decision 0085), the approved draft: the first section of the column. With several
 * accounts "All accounts": the unified inbox, the views the user put there (Later and Flagged by
 * default), Sent; then "More" with the other views, the account groups, the saved searches,
 * Activity and the outbox. With one account "Views", without the inbox and Sent (its own section
 * has them). One tree, one Tab stop; the label folds it, "More" opens and closes (both
 * remembered on this device). Later, Flagged and Sent show the selected account, as in the rail.
 */
export function AllAccountsTree({ accounts }: { accounts: readonly Account[] }) {
  const { t, i18n } = useTranslation();
  const view = useUiStore((state) => state.view);
  const unified = useUiStore((state) => state.unifiedInbox);
  const showMailView = useUiStore((state) => state.showMailView);
  const showUnifiedInbox = useUiStore((state) => state.showUnifiedInbox);
  const showSavedSearch = useUiStore((state) => state.showSavedSearch);
  const showActivity = useUiStore((state) => state.showActivity);
  const showOutbox = useUiStore((state) => state.showOutbox);
  const views = useColumnViews();
  const groups = useColumnGroups();
  const choose = useColumnViewsStore((state) => state.choose);
  const openEditor = useColumnViewsStore((state) => state.openEditor);
  const unifiedEntries = useUnifiedEntries(accounts);
  const searches = useSavedSearches().data ?? [];
  const activity = useActivityEntry(accounts);
  const outbox = useOutboxEntry(accounts);
  const several = accounts.length > 1;
  const section = useSection("allAccounts", several ? t("nav.allAccounts") : t("nav.viewsSection"));
  const more = useFold("more");
  const headingId = useId();
  const [hidden, setHidden] = useState<string | null>(null);

  // The row of the view shown right now.
  const inInbox = view.kind === "mail" && view.mailView === "inbox";
  let currentKey: string | null = null;
  if (view.kind === "activity" || view.kind === "outbox") currentKey = view.kind;
  else if (view.kind === "mail") {
    if (view.mailView === "savedViews" && view.savedSearchId !== undefined) currentKey = `search:${String(view.savedSearchId)}`;
    else if (inInbox && unified?.kind === "group") currentKey = `group:${unified.groupId}`;
    else if (inInbox && several && sameUnifiedView(unified, unifiedEntries.all.view)) currentKey = "inbox";
    else if (view.mailView === "sent" && several) currentKey = "sent";
    else if (view.mailView !== "inbox" && view.mailView !== "sent") currentKey = view.mailView;
  }

  const unreadLabel = (name: string, count: number) => (count > 0 ? t("sidebar.folderUnread", { name, count }) : name);
  // The unified inbox shows as "Inbox" of "All accounts" (the draft); its menu and notices name it.
  const unifiedRow = (entry: UnifiedEntry, key: string, level: number, icon: LucideIcon, emphasis: boolean): Omit<Entry, "item"> & { key: string; text: string; level: number } => ({
    key,
    text: key === "inbox" ? t("nav.inbox") : entry.name,
    level,
    icon,
    label: unreadLabel(key === "inbox" ? t("nav.inbox") : entry.name, entry.unread),
    current: currentKey === key,
    count: <UnreadCount count={entry.unread} emphasis={emphasis} />,
    menu: (
      <UnifiedRowMenu
        name={entry.name}
        targets={entry.targets}
        returnFocus={() => {
          nav.focus(key);
        }}
      />
    ),
    onOpen: () => {
      showUnifiedInbox(entry.view);
    },
    dropInbox: key === "inbox",
    "data-testid": `unified-${entry.key}`,
  });
  const viewRow = (id: ColumnViewId, level: number): Omit<Entry, "item"> & { key: string; text: string; level: number } => ({
    key: id,
    text: t(`nav.${id}`),
    level,
    icon: MAIL_VIEW_ICONS[id],
    label: t(`nav.${id}`),
    current: currentKey === id,
    menu: (
      <ViewRowMenu
        id={id}
        onHide={() => {
          hide(id);
        }}
        returnFocus={() => {
          nav.focus(id);
        }}
      />
    ),
    onOpen: () => {
      showMailView(id);
    },
    "data-testid": `sidebar-view-${id}`,
  });

  type Draft = Omit<Entry, "item"> & { key: string; text: string; level: number };
  const top: Draft[] = [
    ...(several ? [unifiedRow(unifiedEntries.all, "inbox", 2, Inbox, true)] : []),
    ...groups.main.map((id) => viewRow(id, 2)),
    ...(several
      ? [
          {
            key: "sent",
            text: t("nav.sent"),
            level: 2,
            icon: Send,
            label: t("nav.sent"),
            current: currentKey === "sent",
            onOpen: () => {
              showMailView("sent");
            },
            "data-testid": "sidebar-view-sent",
          },
        ]
      : []),
  ];
  const inside: Draft[] = [
    ...groups.more.map((id) => viewRow(id, 3)),
    ...unifiedEntries.groups.map((entry) => unifiedRow(entry, `group:${entry.key}`, 3, Layers, false)),
    ...searches.map((search) => ({
      key: `search:${String(search.id)}`,
      text: search.name,
      level: 3,
      icon: Search,
      label: search.name,
      current: currentKey === `search:${String(search.id)}`,
      onOpen: () => {
        showSavedSearch(search.id);
      },
      "data-testid": "saved-search-nav-entry",
    })),
    {
      key: "activity",
      text: activity.text,
      level: 3,
      icon: History,
      label: activity.label,
      current: currentKey === "activity",
      count: <StatusCount entry={activity} data-testid="activity-badge" />,
      onOpen: showActivity,
      "data-testid": "sidebar-activity",
    },
    {
      key: "outbox",
      text: outbox.text,
      level: 3,
      icon: SendHorizontal,
      label: outbox.label,
      current: currentKey === "outbox",
      count: <StatusCount entry={outbox} data-testid="outbox-badge" />,
      onOpen: showOutbox,
      "data-testid": "sidebar-outbox",
    },
  ];
  // While "More" is closed: the view shown inside it marks "More", and what needs the user
  // (failed actions or sends) shows on it, so nothing that matters hides there.
  const insideCurrent = inside.some((entry) => entry.current);
  const attention = activity.attention + outbox.attention;
  const attentionEntry: StatusEntry = { pending: 0, attention, label: "", text: "", count: attention > 0 ? new Intl.NumberFormat(i18n.language).format(attention) : null };
  const moreDraft: Draft = {
    key: "more",
    text: t("nav.more"),
    level: 2,
    icon: more.open ? ChevronDown : ChevronRight,
    label: attention > 0 && !more.open ? `${t("nav.more")}, ${activity.attention > 0 ? activity.label : outbox.label}` : t("nav.more"),
    current: false,
    filled: !more.open && insideCurrent,
    quiet: true,
    count: more.open ? undefined : <StatusCount entry={attentionEntry} data-testid="more-attention" />,
    onOpen: more.toggle,
    "data-testid": "sidebar-more",
  };
  const shownDrafts = section.open ? [...top, moreDraft, ...(more.open ? inside : [])] : [];
  const level2 = section.open ? [...top, moreDraft] : [];
  const entries: Entry[] = shownDrafts.map((draft) => {
    const siblings = draft.level === 2 ? level2 : inside;
    const { key, text, level, ...rest } = draft;
    return {
      ...rest,
      item: {
        key,
        level,
        text,
        posinset: siblings.findIndex((sibling) => sibling.key === key) + 1,
        setsize: siblings.length,
        expanded: key === "more" ? more.open : undefined,
      },
    };
  });
  const nav = useTreeNavigation({
    items: [section.item, ...entries.map((entry) => entry.item)],
    currentKey: currentKey !== null && !more.open && inside.some((entry) => entry.key === currentKey) ? "more" : currentKey,
    onOpen: (key) => {
      if (key === SECTION_KEY) section.toggle();
      entries.find((entry) => entry.item.key === key)?.onOpen();
    },
    onExpand: (key, expanded) => {
      section.onExpand(key, expanded);
      if (key === "more") more.setOpen(expanded);
    },
  });
  const hide = (id: ColumnViewId) => {
    const order = [...groups.main, ...groups.more];
    const index = order.indexOf(id);
    // The focus goes to the view after it, else the one before, else the label.
    const next = order[index + 1] ?? order[index - 1] ?? SECTION_KEY;
    flushSync(() => {
      choose(views.map((entry) => (entry.id === id ? { ...entry, shown: false } : entry)));
    });
    nav.focus(entries.some((entry) => entry.item.key === next) ? next : SECTION_KEY);
    setHidden(t("columnViews.hidden", { name: t(`nav.${id}`) }));
  };
  const labelMenu: OverflowItem[] = [
    { id: "edit", label: t("columnViews.edit"), icon: SlidersHorizontal, onSelect: openEditor, "data-testid": "sidebar-views-edit" },
  ];
  const attentionFolded = <StatusCount entry={attentionEntry} data-testid="section-attention" />;
  return (
    <section aria-labelledby={headingId} className="flex flex-col" data-testid="sidebar-all-accounts">
      <h2 id={headingId} className="sr-only">
        {section.item.text}
      </h2>
      <ul {...nav.treeProps} aria-labelledby={headingId} className="flex flex-col" data-testid="sidebar-views">
        <SectionRow
          section={section}
          nav={nav}
          folded={attention > 0 ? attentionFolded : undefined}
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
        {entries.map((entry) => (
          <EntryRow key={entry.item.key} entry={entry} nav={nav} />
        ))}
      </ul>
      {/* Stage 17: every view hidden: a quiet hint where they would be. */}
      {section.open && groups.main.length === 0 && groups.more.length === 0 && (
        <div className="flex flex-col items-start gap-1 px-2 pt-1" data-testid="sidebar-views-empty">
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
