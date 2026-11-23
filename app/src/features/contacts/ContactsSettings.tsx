import { useId, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import type { ContactEntry } from "../../services/ipc";
import { ContactDuplicates } from "./ContactDuplicates";
import { contactName } from "./names";
import { useContactDirectory, useDebouncedQuery, useSeparateContact, useSetContactName } from "./queries";

function NameEditor({ contact, onDone }: { contact: ContactEntry; onDone: () => void }) {
  const { t } = useTranslation();
  const id = useId();
  const hintId = useId();
  const [value, setValue] = useState(contact.userName ?? contact.name ?? "");
  const rename = useSetContactName();
  const save = (name: string | null) => {
    rename.mutate(
      { email: contact.email, name: name?.trim() ? name.trim() : null },
      {
        onSuccess: onDone,
      },
    );
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      save(value);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onDone();
    }
  };
  return (
    <div className="flex flex-col gap-2" data-testid="contact-name-editor">
      <label htmlFor={id} className="text-13 font-medium text-primary">
        {t("contacts.rename.label")}
      </label>
      <input
        id={id}
        value={value}
        autoFocus
        onChange={(e) => {
          setValue(e.target.value);
        }}
        onKeyDown={onKeyDown}
        aria-describedby={hintId}
        className="h-8 w-full rounded-control border border-border-strong bg-surface-raised px-3 text-14 text-primary"
        data-testid="contact-name-input"
      />
      <p id={hintId} className="text-12 text-secondary">
        {t("contacts.rename.hint")}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          disabled={rename.isPending}
          onClick={() => {
            save(value);
          }}
          data-testid="contact-name-save"
        >
          {t("contacts.rename.save")}
        </Button>
        {contact.userName !== null && (
          <Button
            variant="quiet"
            disabled={rename.isPending}
            onClick={() => {
              save(null);
            }}
            data-testid="contact-name-reset"
          >
            {t("contacts.rename.reset")}
          </Button>
        )}
        <Button variant="quiet" onClick={onDone} data-testid="contact-name-cancel">
          {t("contacts.rename.cancel")}
        </Button>
      </div>
      {rename.isError && <ErrorNotice context={t("contacts.rename.failed")} error={rename.error} data-testid="contact-name-error" />}
    </div>
  );
}

function ContactRow({ contact }: { contact: ContactEntry }) {
  const { t, i18n } = useTranslation();
  const [editing, setEditing] = useState(false);
  const separate = useSeparateContact();
  const date = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" });
  const others = contact.addresses.slice(1);
  const name = contactName(contact, t("contacts.noName"));
  return (
    <li className="flex flex-col gap-2 border-b border-border py-3" data-testid="contact-row" data-contact={contact.key}>
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
        <p className="min-w-0 truncate text-14 font-medium text-primary" data-testid="contact-name">
          {name}
        </p>
        {contact.userName !== null && (
          <span className="shrink-0 rounded-control bg-accent-soft px-1 text-12 text-primary" data-testid="contact-own-name">
            {t("contacts.ownName")}
          </span>
        )}
        <p className="min-w-0 truncate text-13 text-secondary" data-testid="contact-email">
          {contact.email}
        </p>
      </div>
      <p className="text-12 text-secondary tabular-nums" data-testid="contact-usage">
        {[
          t("contacts.sent", { count: contact.sent }),
          t("contacts.received", { count: contact.received }),
          ...(contact.lastUsed !== null ? [t("contacts.lastUsed", { date: date.format(contact.lastUsed) })] : []),
        ].join(" · ")}
      </p>
      {others.length > 0 && (
        <div className="flex flex-col gap-1">
          <p className="text-12 font-medium text-secondary">{t("contacts.otherAddresses")}</p>
          <ul aria-label={t("contacts.otherAddressesOf", { name })} className="flex flex-col gap-1">
            {others.map((address) => (
              <li key={address.email} className="flex flex-wrap items-center gap-2 text-13 text-primary" data-testid="contact-address">
                <span className="min-w-0 truncate">{address.email}</span>
                <Button
                  variant="quiet"
                  disabled={separate.isPending}
                  aria-label={t("contacts.separate.label", { email: address.email })}
                  onClick={() => {
                    separate.mutate(address.email);
                  }}
                  data-testid="contact-separate"
                >
                  {t("contacts.separate.button")}
                </Button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {separate.isError && <ErrorNotice context={t("contacts.separate.failed")} error={separate.error} data-testid="contact-separate-error" />}
      {editing ? (
        <NameEditor
          contact={contact}
          onDone={() => {
            setEditing(false);
          }}
        />
      ) : (
        <div>
          <Button
            variant="quiet"
            aria-label={t("contacts.rename.openLabel", { email: contact.email })}
            onClick={() => {
              setEditing(true);
            }}
            data-testid="contact-rename"
          >
            {t("contacts.rename.open")}
          </Button>
        </div>
      )}
    </li>
  );
}

/**
 * Settings → Contacts (stage 10): the local directory, most used first, names the user can
 * change, and duplicate suggestions that are merged only after an explicit confirmation.
 */
export function ContactsSettings() {
  const { t } = useTranslation();
  const searchId = useId();
  const [search, setSearch] = useState("");
  const query = useDebouncedQuery(search);
  const directory = useContactDirectory(query);
  const contacts = directory.data?.pages.flatMap((page) => page.items) ?? [];

  let body;
  if (directory.isError && contacts.length === 0) {
    body = (
      <ErrorNotice
        context={t("contacts.loadFailed")}
        error={directory.error}
        onRetry={() => void directory.refetch()}
        data-testid="contacts-error"
      />
    );
  } else if (directory.isPending) {
    body = (
      <p role="status" className="text-13 text-secondary">
        {t("contacts.loading")}
      </p>
    );
  } else if (contacts.length === 0) {
    body =
      query === "" ? (
        <div className="flex flex-col gap-1 rounded-card border border-border p-4" data-testid="contacts-empty">
          <p className="text-14 font-medium text-primary">{t("contacts.emptyTitle")}</p>
          <p className="text-13 text-secondary">{t("contacts.emptyBody")}</p>
        </div>
      ) : (
        <p role="status" className="text-13 text-secondary" data-testid="contacts-no-match">
          {t("contacts.noMatch", { query })}
        </p>
      );
  } else {
    body = (
      <>
        <ul aria-label={t("contacts.list")} className="flex flex-col" data-testid="contacts-list">
          {contacts.map((contact) => (
            <ContactRow key={contact.key} contact={contact} />
          ))}
        </ul>
        {directory.isError && (
          <ErrorNotice context={t("contacts.loadFailed")} error={directory.error} onRetry={() => void directory.fetchNextPage()} />
        )}
        {directory.hasNextPage && (
          <div>
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
    <SettingsSection title={t("settings.contacts.title")}>
      <p className="text-13 text-secondary">{t("contacts.intro")}</p>
      <ContactDuplicates />
      <div className="flex flex-col gap-1">
        <label htmlFor={searchId} className="text-13 font-medium text-primary">
          {t("contacts.search")}
        </label>
        <input
          id={searchId}
          type="search"
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
          }}
          autoComplete="off"
          spellCheck={false}
          className="h-8 w-full rounded-control border border-border-strong bg-surface-raised px-3 text-14 text-primary"
          data-testid="contacts-search"
        />
      </div>
      <div className="flex flex-col gap-3">{body}</div>
    </SettingsSection>
  );
}
