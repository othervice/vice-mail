import { UserRound } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import type { ContactEntry } from "../../services/ipc";

interface ContactSuggestionsProps {
  /** Id of the listbox; the search field names it in `aria-controls`. */
  id: string;
  contacts: readonly ContactEntry[];
  /** The option the arrow keys moved to (-1: none); the field points at it with `aria-activedescendant`. */
  active: number;
  optionId: (index: number) => string;
  onPick: (contact: ContactEntry) => void;
}

/**
 * Stage 22 (decision 0074): up to three contacts on top while a search is typed. Choosing one
 * shows all mail from, to and in copy of the person, any of their addresses, over all accounts.
 * The focus stays in the field (combobox): the arrow keys move through the list, Enter chooses,
 * Escape closes it; the full-text results stay below. A click chooses too (the mouse down does
 * not take the focus from the field).
 */
export function ContactSuggestions({ id, contacts, active, optionId, onPick }: ContactSuggestionsProps) {
  const { t } = useTranslation();
  return (
    <div
      className="absolute inset-x-0 top-full z-20 mt-1 flex flex-col overflow-hidden rounded-card border border-border-strong bg-surface-raised shadow-floating"
      data-testid="search-contacts"
    >
      <p aria-hidden className="px-3 pt-2 pb-1 text-12 font-medium text-secondary">
        {t("search.contacts.heading")}
      </p>
      <ul id={id} role="listbox" aria-label={t("search.contacts.label")} className="flex flex-col pb-1">
        {contacts.map((contact, index) => {
          const name = contact.name ?? contact.email;
          const more = contact.addresses.length - 1;
          return (
            <li
              key={contact.key}
              id={optionId(index)}
              role="option"
              aria-selected={index === active}
              aria-label={t("search.contacts.option", { name, email: contact.email })}
              onMouseDown={(event) => {
                event.preventDefault();
              }}
              onClick={() => {
                onPick(contact);
              }}
              className={cx(
                "flex min-h-10 cursor-default items-center gap-2 px-3 py-1 text-13 text-primary",
                index === active ? "bg-selected" : "hover:bg-hover",
              )}
              data-testid="search-contact-option"
            >
              <UserRound aria-hidden className="size-(--ui-icon-small) shrink-0 text-tertiary" strokeWidth={1.75} />
              <span className="flex min-w-0 flex-col">
                <span className="truncate">{name}</span>
                <span className="truncate text-12 text-secondary">
                  {more > 0 ? t("search.contacts.moreAddresses", { email: contact.email, count: more }) : contact.email}
                </span>
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
