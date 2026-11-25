import { BookUser, Search, UserRound } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ViewHeader } from "../../components/ViewHeader";
import type { ContactEntry } from "../../services/ipc";
import { Avatar } from "../accounts/Avatar";
import { useWindowDragRegion } from "../app-info/useWindowDragRegion";
import { dayBounds, formatFullDate, formatListDate } from "../messages/rows";
import { ContactBadges, ContactDetail } from "./ContactDetail";
import { contactName } from "./names";
import { useContactDirectory, useDebouncedQuery } from "./queries";
// Stage 19 (decision 0056): contact groups, vCard import and export.
import type { ContactListId } from "../../services/ipc";
import { ContactsTransfer } from "./groups/ContactsTransfer";
import { GroupDetail, GroupList, NoGroup } from "./groups/GroupsPane";
import { useContactLists } from "./groups/queries";

/** Stage 19: what the list column shows. */
const CONTACT_TABS = ["contacts", "groups"] as const;

/** Search over 10,000 contacts answers in a few ms; a short pause is enough. */
const SEARCH_DEBOUNCE_MS = 120;

function optionId(listId: string, key: string): string {
  return `${listId}-${key.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

function ContactOption({
  contact,
  id,
  selected,
  active,
  onSelect,
}: {
  contact: ContactEntry;
  id: string;
  selected: boolean;
  active: boolean;
  onSelect: () => void;
}) {
  const { t, i18n } = useTranslation();
  const name = contactName(contact, t("contacts.noName"));
  const bounds = dayBounds(new Date(), 1);
  return (
    <div
      id={id}
      role="option"
      aria-selected={selected}
      onClick={onSelect}
      data-active={active || undefined}
      data-testid="contact-row"
      data-contact={contact.key}
      className={cx(
        "mx-2 flex cursor-default items-center gap-3 rounded-row px-2 py-2",
        selected ? "bg-selected" : "hover:bg-hover",
        active && "outline-2 -outline-offset-2 outline-focus-ring",
      )}
    >
      <Avatar name={contact.name} email={contact.email} accountColor={0} ring={false} />
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 truncate text-14 font-medium text-primary" data-testid="contact-name">
            {name}
          </span>
          <ContactBadges contact={contact} />
        </div>
        <span className="truncate text-12 text-secondary" data-testid="contact-email">
          {contact.email}
        </span>
      </div>
      {contact.lastUsed !== null && (
        <time
          dateTime={new Date(contact.lastUsed).toISOString()}
          title={formatFullDate(contact.lastUsed, i18n.language)}
          className="shrink-0 self-start pt-1 text-12 text-tertiary tabular-nums"
          data-testid="contact-last-used"
        >
          {formatListDate(contact.lastUsed, bounds, i18n.language)}
        </time>
      )}
    </div>
  );
}

/**
 * Stage 17 (decision 0050): the contacts as a view of their own ("Contacts" in the column, the
 * rail and the palette). On the left every contact with picture, name, address and the date of
 * the last mail, most used first, with a search over name, address and domain and, on request,
 * the list and system addresses; on the right the chosen contact. Only what the core built from
 * mail on this device: without mail it says so, never with examples. Keyboard: the search field,
 * then the list (arrow keys, Home, End; Enter goes to the contact), then the contact's controls.
 */
export function ContactsView() {
  const { t } = useTranslation();
  const dragRegion = useWindowDragRegion();
  const searchId = useId();
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const [search, setSearch] = useState("");
  const [all, setAll] = useState(false);
  const query = useDebouncedQuery(search, SEARCH_DEBOUNCE_MS);
  const directory = useContactDirectory(query, all ? "all" : "people");
  const contacts = directory.data?.pages.flatMap((page) => page.items) ?? [];
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [focusRequest, setFocusRequest] = useState(0);
  const [listFocused, setListFocused] = useState(false);
  // Stage 19: people or groups on the left.
  const [tab, setTab] = useState<"contacts" | "groups">("contacts");
  const [groupId, setGroupId] = useState<ContactListId | null>(null);
  const groups = useContactLists().data ?? [];
  const group = groups.find((g) => g.id === groupId) ?? groups[0];
  const tabsId = useId();

  // The first contact is the choice until the user picks one (and again when it drops out).
  const picked = contacts.findIndex((c) => c.key === selectedKey);
  const selectedIndex = picked >= 0 ? picked : contacts.length > 0 ? 0 : -1;
  const selected = selectedIndex >= 0 ? contacts[selectedIndex] : undefined;

  // The chosen row stays in view while the arrow keys move.
  useEffect(() => {
    if (!selected) return;
    document.getElementById(optionId(listId, selected.key))?.scrollIntoView?.({ block: "nearest" });
  }, [selected, listId]);

  const choose = (index: number) => {
    const next = contacts[Math.max(0, Math.min(contacts.length - 1, index))];
    if (next) setSelectedKey(next.key);
  };

  const onListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const current = Math.max(0, selectedIndex);
    const moves: Record<string, number> = {
      ArrowDown: current + 1,
      ArrowUp: current - 1,
      Home: 0,
      End: contacts.length - 1,
      PageDown: current + 10,
      PageUp: current - 10,
    };
    const target = moves[event.key];
    if (target !== undefined) {
      event.preventDefault();
      choose(target);
      if (event.key === "End" || target >= contacts.length - 1) {
        if (directory.hasNextPage && !directory.isFetchingNextPage) void directory.fetchNextPage();
      }
    } else if (event.key === "Enter" && selected) {
      event.preventDefault();
      setFocusRequest((n) => n + 1);
    }
  };

  const onSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" && contacts.length > 0) {
      event.preventDefault();
      listRef.current?.focus();
    } else if (event.key === "Escape" && search !== "") {
      event.preventDefault();
      setSearch("");
    }
  };

  let list;
  if (directory.isError && contacts.length === 0) {
    list = (
      <div className="px-4 py-3">
        <ErrorNotice context={t("contacts.loadFailed")} error={directory.error} onRetry={() => void directory.refetch()} data-testid="contacts-error" />
      </div>
    );
  } else if (directory.isPending) {
    list = (
      <p role="status" className="px-4 py-3 text-13 text-secondary">
        {t("contacts.loading")}
      </p>
    );
  } else if (contacts.length === 0) {
    list =
      query === "" ? (
        <div className="px-4 py-6 text-center" data-testid="contacts-empty">
          <p className="text-14 font-medium text-primary">{t("contacts.emptyTitle")}</p>
          <p className="text-13 text-secondary">{all ? t("contacts.emptyBody") : t("contacts.view.emptyPeople")}</p>
        </div>
      ) : (
        <p role="status" className="px-4 py-3 text-13 text-secondary" data-testid="contacts-no-match">
          {t("contacts.noMatch", { query })}
        </p>
      );
  } else {
    list = (
      <>
        <div
          ref={listRef}
          id={listId}
          role="listbox"
          tabIndex={0}
          aria-label={t("contacts.list")}
          aria-activedescendant={selected ? optionId(listId, selected.key) : undefined}
          onKeyDown={onListKeyDown}
          onFocus={() => {
            setListFocused(true);
          }}
          onBlur={() => {
            setListFocused(false);
          }}
          className="flex flex-col py-1 focus:outline-none"
          data-testid="contacts-list"
        >
          {contacts.map((contact) => (
            <ContactOption
              key={contact.key}
              contact={contact}
              id={optionId(listId, contact.key)}
              selected={contact.key === selected?.key}
              active={listFocused && contact.key === selected?.key}
              onSelect={() => {
                setSelectedKey(contact.key);
                listRef.current?.focus();
              }}
            />
          ))}
        </div>
        {directory.isError && (
          <div className="px-4 py-2">
            <ErrorNotice context={t("contacts.loadFailed")} error={directory.error} onRetry={() => void directory.fetchNextPage()} />
          </div>
        )}
        {directory.hasNextPage && (
          <div className="px-4 py-3">
            <Button
              variant="secondary"
              disabled={directory.isFetchingNextPage}
              onClick={() => void directory.fetchNextPage()}
              data-testid="contacts-load-more"
            >
              {t("contacts.loadMore")}
            </Button>
          </div>
        )}
      </>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="contacts-view">
      <ViewHeader title={t("nav.contacts")} subtitle={<span className="text-13 text-secondary">{t("contacts.view.subtitle")}</span>} dragRegion={dragRegion}>
        <ContactsTransfer
          contact={tab === "contacts" && selected ? { email: selected.email, name: contactName(selected, selected.email) } : null}
          group={tab === "groups" && group ? { id: group.id, name: group.name } : null}
        />
      </ViewHeader>
      <div className="flex min-h-0 flex-1">
        <section
          aria-label={t("contacts.view.listSection")}
          className="flex w-(--ui-contacts-list-width) min-w-0 shrink-0 flex-col border-r border-border-subtle bg-surface-list"
        >
          <div role="tablist" aria-label={t("contactGroups.tabs.label")} className="flex gap-1 border-b border-border-subtle px-4 pt-3 pb-2">
            {CONTACT_TABS.map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`${tabsId}-${id}`}
                aria-selected={tab === id}
                tabIndex={tab === id ? 0 : -1}
                onClick={() => {
                  setTab(id);
                }}
                onKeyDown={(event) => {
                  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
                    event.preventDefault();
                    const next = id === "contacts" ? "groups" : "contacts";
                    setTab(next);
                    document.getElementById(`${tabsId}-${next}`)?.focus();
                  }
                }}
                className={cx(
                  "rounded-control px-3 py-1 text-13 font-medium",
                  tab === id ? "bg-selected text-primary" : "text-secondary hover:bg-hover hover:text-primary",
                )}
                data-testid={`contacts-tab-${id}`}
              >
                {t(`contactGroups.tabs.${id}`)}
              </button>
            ))}
          </div>
          {tab === "groups" ? (
            <GroupList selected={group?.id ?? null} onSelect={setGroupId} />
          ) : (
          <>
          <div className="flex flex-col gap-2 border-b border-border-subtle px-4 py-3">
            <label htmlFor={searchId} className="sr-only">
              {t("contacts.search")}
            </label>
            <div className="relative">
              <Search aria-hidden className="pointer-events-none absolute top-2 left-2 size-(--ui-icon-small) text-tertiary" strokeWidth={1.75} />
              <input
                id={searchId}
                type="search"
                value={search}
                placeholder={t("contacts.view.searchPlaceholder")}
                onChange={(event) => {
                  setSearch(event.target.value);
                }}
                onKeyDown={onSearchKeyDown}
                autoComplete="off"
                spellCheck={false}
                className="h-8 w-full rounded-control border border-border bg-surface-raised ps-8 pe-3 text-14 text-primary placeholder:text-tertiary"
                data-testid="contacts-search"
              />
            </div>
            <label className="flex cursor-default items-center gap-2 text-12 text-secondary">
              <input
                type="checkbox"
                checked={all}
                onChange={(event) => {
                  setAll(event.target.checked);
                }}
                className="size-4 shrink-0 accent-accent"
                data-testid="contacts-show-all"
              />
              {t("contacts.view.showAutomated")}
            </label>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto" aria-busy={directory.isFetching || undefined}>
            {list}
          </div>
          </>
          )}
        </section>
        <section aria-label={t("contacts.view.detailSection")} className="min-w-0 flex-1 overflow-y-auto bg-surface">
          {tab === "groups" ? (
            group ? (
              <GroupDetail
                key={group.id}
                id={group.id}
                onDeleted={() => {
                  setGroupId(null);
                }}
              />
            ) : (
              <NoGroup />
            )
          ) : selected ? (
            <ContactDetail key={selected.key} entry={selected} focusRequest={focusRequest} />
          ) : contacts.length === 0 && query === "" && !directory.isPending ? (
            <EmptyState icon={BookUser} title={t("contacts.view.noContactsTitle")} body={t("contacts.view.noContactsBody")} />
          ) : (
            <EmptyState icon={UserRound} title={t("contacts.view.noSelectionTitle")} body={t("contacts.view.noSelectionBody")} />
          )}
        </section>
      </div>
    </div>
  );
}
