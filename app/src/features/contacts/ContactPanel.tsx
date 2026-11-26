import { useQueries } from "@tanstack/react-query";
import { useEffect, useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { contactListIpc, toCoreError, type ContactEntry, type MessageListItem } from "../../services/ipc";
import { openComposer } from "../../stores/composer";
import { useContactPanelStore, type PanelPerson } from "../../stores/contactPanel";
import { useLayoutStore } from "../../stores/layout";
import { useSearchStore } from "../../stores/search";
import { useUiStore } from "../../stores/ui";
import { useAiPanelShown } from "../ai/AiPanel";
import { dayBounds, formatFullDate, formatListDate } from "../messages/rows";
import { openMessageInWindow } from "../windows/open";
import { contactListQueryKey, useContactLists } from "./groups/queries";
import { contactName } from "./names";
import { useContact, useContactMessages } from "./queries";

/** The panel's id, for the toolbar's toggle (`aria-controls`). */
export const CONTACT_PANEL_ID = "contact-panel";

/** Mails the panel lists, newest first (the contacts view shows more). */
const PANEL_MAILS = 4;

/** Widths the window needs besides the panel, in px at 100 % text size (docs/DESIGN.md). */
const RAIL = 56;
const LIST_MIN = 320;
const READING_MIN = 360;
const PANEL = 264;
const AI_PANEL = 380;
const AI_PANEL_OVERLAY_BELOW = 1100;

/**
 * Whether the window has room for the contact panel next to the sidebar (or the rail), the
 * list's and the reading pane's smallest widths and the AI panel when it is open. Not on narrow
 * windows: the panel hides and its toggle with it; the stored choice stays.
 */
export function useContactPanelFits(): boolean {
  const docked = useUiStore((state) => state.sidebarExpanded && !state.sidebarNarrow);
  const sidebarWidth = useUiStore((state) => state.sidebarWidth);
  const aiShown = useAiPanelShown();
  const [width, setWidth] = useState(() => window.innerWidth);
  useEffect(() => {
    const update = () => {
      setWidth(window.innerWidth);
    };
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("resize", update);
    };
  }, []);
  const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  // Below 1100 px the AI panel lies over the content (AiPanel) and takes no room of its own.
  const aiRoom = aiShown && width > AI_PANEL_OVERLAY_BELOW ? AI_PANEL : 0;
  const needed = ((docked ? sidebarWidth : RAIL) + LIST_MIN + READING_MIN + PANEL + aiRoom) * (rem / 16);
  return width >= needed;
}

/** The panel is on screen: chosen, a mail view, and room for it. */
export function useContactPanelShown(): boolean {
  const chosen = useLayoutStore((state) => state.contactPanel);
  const mailView = useUiStore((state) => state.view.kind === "mailbox" || (state.view.kind === "mail" && state.view.mailView !== "contacts"));
  const fits = useContactPanelFits();
  return chosen && mailView && fits;
}

function Section({ title, children, ...rest }: { title: string; children: ReactNode; "data-testid"?: string }) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-2" data-testid={rest["data-testid"]}>
      <h3 id={id} className="text-11 font-semibold text-tertiary">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** The newest mail with the person: subject and day, a click opens it in a window. */
function RecentMails({ email }: { email: string }) {
  const { t, i18n } = useTranslation();
  const messages = useContactMessages(email);
  const bounds = dayBounds(new Date(), 1);
  if (messages.isPending) {
    return (
      <p role="status" className="text-12 text-tertiary">
        {t("contacts.view.recentLoading")}
      </p>
    );
  }
  if (messages.isError) {
    return <ErrorNotice context={t("contacts.view.recentFailed")} error={messages.error} onRetry={() => void messages.refetch()} />;
  }
  const items = messages.data.items.slice(0, PANEL_MAILS);
  if (items.length === 0) {
    return <p className="text-12 text-tertiary">{t("contacts.view.recentEmpty")}</p>;
  }
  const subjectOf = (item: MessageListItem) => (item.subject.trim() ? item.subject : t("list.noSubject"));
  return (
    <ul aria-label={t("contactPanel.mails")} className="flex flex-col gap-1">
      {items.map((item) => (
        <li key={item.id}>
          <button
            type="button"
            onClick={() => {
              openMessageInWindow(item.accountId, item.id);
            }}
            className="-mx-2 flex w-[calc(100%+var(--ui-space-4))] min-w-0 flex-col rounded-control px-2 py-1 text-start transition-colors hover:bg-hover"
            data-testid="contact-panel-mail"
          >
            <span className="w-full truncate text-12 font-semibold text-primary">{subjectOf(item)}</span>
            <time
              dateTime={new Date(item.date).toISOString()}
              title={formatFullDate(item.date, i18n.language)}
              className="text-11 text-tertiary tabular-nums"
            >
              {formatListDate(item.date, bounds, i18n.language)}
            </time>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The groups (stage 19) one of the contact's addresses belongs to. */
function useGroupsOf(addresses: readonly string[]): { names: string[]; pending: boolean } {
  const lists = useContactLists();
  const ids = (lists.data ?? []).filter((list) => list.memberCount > 0).map((list) => list.id);
  const results = useQueries({
    queries: ids.map((id) => ({ queryKey: contactListQueryKey(id), queryFn: () => contactListIpc.get(id) })),
  });
  const wanted = new Set(addresses.map((address) => address.trim().toLowerCase()));
  const names = results.flatMap((result) =>
    result.data && result.data.members.some((member) => wanted.has(member.email.trim().toLowerCase())) ? [result.data.name] : [],
  );
  return { names, pending: lists.isPending || results.some((result) => result.isPending) };
}

function Groups({ contact }: { contact: ContactEntry }) {
  const { t } = useTranslation();
  const groups = useGroupsOf(contact.addresses.map((address) => address.email));
  if (groups.pending || groups.names.length === 0) return null;
  return (
    <Section title={t("contactPanel.groups")} data-testid="contact-panel-groups">
      <ul className="flex flex-wrap gap-1">
        {groups.names.map((name) => (
          <li key={name} className="rounded-full bg-badge px-2 text-12 leading-5 text-secondary">
            {name}
          </li>
        ))}
      </ul>
    </Section>
  );
}

function PersonDetails({ person }: { person: PanelPerson }) {
  const { t } = useTranslation();
  const query = useContact(person.email);
  const showMailView = useUiStore((state) => state.showMailView);
  // No contact yet (a first mail): the message's name and address, no note, no groups.
  const missing = query.isError && toCoreError(query.error).code === "not_found";
  const contact = query.data;
  const email = contact?.email ?? person.email;
  // The contact's name, else the one the message gives (a calendar's "Calendar"), else the address.
  const name = (contact?.name?.trim() ? contactName(contact, contact.email) : null) ?? (person.name?.trim() || email);
  const note = contact?.note?.trim();

  const writeMail = () => {
    openComposer({ kind: "new", accountId: person.accountId, to: [{ name: contact?.name ?? person.name, email }] });
  };
  // Stage 22 (decision 0074): every mail with the contact, from and to any of its addresses.
  const showAllMail = () => {
    const search = useSearchStore.getState();
    showMailView("inbox");
    search.setScope("all");
    search.setQuery(`${t("search.contacts.key")}:${email}`);
    search.openSearch();
    search.submit();
  };

  return (
    <>
      <div className="flex min-w-0 flex-col gap-1">
        <h2 className="text-16 leading-6 font-bold break-words text-primary select-text" data-testid="contact-panel-name">
          {name}
        </h2>
        <p className="truncate text-12 text-tertiary select-text" title={email} data-testid="contact-panel-email">
          {email}
        </p>
      </div>
      <div role="group" aria-label={t("contacts.view.actions", { name })} className="flex gap-2">
        <button
          type="button"
          onClick={writeMail}
          className="h-(--ui-toolbar-control) min-w-0 flex-1 truncate rounded-control border border-border bg-button px-2 text-12 font-medium text-primary transition-colors hover:bg-hover"
          data-testid="contact-panel-write"
        >
          {t("contactPanel.write")}
        </button>
        <button
          type="button"
          onClick={showAllMail}
          className="h-(--ui-toolbar-control) min-w-0 flex-1 truncate rounded-control border border-border bg-button px-2 text-12 font-medium text-primary transition-colors hover:bg-hover"
          data-testid="contact-panel-all"
        >
          {t("contactPanel.allMails")}
        </button>
      </div>
      {query.isError && !missing && (
        <ErrorNotice context={t("contactPanel.failed")} error={query.error} onRetry={() => void query.refetch()} />
      )}
      {note && (
        <Section title={t("contacts.view.note")} data-testid="contact-panel-note">
          <p className="text-13 leading-5 break-words whitespace-pre-wrap text-secondary select-text">{note}</p>
        </Section>
      )}
      {/* The contacts store knows the mail of its contacts only (a first sender has none yet). */}
      {contact && (
        <Section title={t("contactPanel.mails")} data-testid="contact-panel-mails">
          <RecentMails email={email} />
        </Section>
      )}
      {contact && <Groups contact={contact} />}
    </>
  );
}

/**
 * Stage 23, step 3 (decision 0084): the contact panel right of the reading pane (264 px, on the
 * `panel` surface), shown and hidden from the toolbar, remembered with the window's layout on this
 * device. It shows the other side of the open message: name, address, "Write mail", "All mail"
 * (the `contact:` search of stage 22), the note, the newest mail and the groups. Everything comes
 * from the contacts store of this computer: no pictures, no network (decision 0072).
 */
export function ContactPanel() {
  const { t } = useTranslation();
  const shown = useContactPanelShown();
  const person = useContactPanelStore((state) => state.person);
  if (!shown) return null;
  return (
    <aside
      id={CONTACT_PANEL_ID}
      aria-label={t("contactPanel.label")}
      className="flex w-(--ui-contact-panel-width) shrink-0 flex-col gap-5 overflow-y-auto border-s border-border-subtle bg-panel px-5 py-6 [scrollbar-width:thin]"
      data-testid="contact-panel"
    >
      {person ? (
        <PersonDetails key={person.email} person={person} />
      ) : (
        <p className="text-12 text-tertiary" data-testid="contact-panel-empty">
          {t("contactPanel.empty")}
        </p>
      )}
    </aside>
  );
}
