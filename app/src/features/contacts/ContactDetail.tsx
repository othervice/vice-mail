import { useQueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { ImagePlus, Search, ShieldAlert, ShieldCheck, SquarePen, Trash2, UserPen } from "lucide-react";
import { useEffect, useEffectEvent, useId, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import {
  composeIpc,
  contactsIpc,
  toCoreError,
  type ContactEntry,
  type ContactPictureSource,
  type CoreError,
  type MessageListItem,
} from "../../services/ipc";
import { openComposer, useComposerStore } from "../../stores/composer";
import { useSearchStore } from "../../stores/search";
import { useUiStore } from "../../stores/ui";
import { Avatar } from "../accounts/Avatar";
import { useSelectedAccount } from "../accounts/selection";
import { dayBounds, formatFullDate, formatListDate } from "../messages/rows";
import { openMessageInWindow } from "../windows/open";
import { NameEditor } from "./NameEditor";
import { contactName } from "./names";
import { PictureEditor } from "./PictureEditor";
import {
  refreshContacts,
  useContact,
  useContactMessages,
  useRemoveContactPicture,
  useSeparateContact,
  useSetContactNote,
  useSetContactPicture,
} from "./queries";
import { useContactSpam } from "./spam";

function Section({ title, children, ...rest }: { title: string; children: ReactNode; "data-testid": string }) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-3" data-testid={rest["data-testid"]}>
      <h3 id={id} className="text-14 font-semibold text-primary">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** Kind and spam mark as small pills next to the name. */
export function ContactBadges({ contact }: { contact: ContactEntry }) {
  const { t } = useTranslation();
  return (
    <>
      {contact.kind !== "person" && (
        <span className="shrink-0 rounded-full bg-badge px-2 text-12 text-secondary" data-testid="contact-kind">
          {t(`contacts.view.kind.${contact.kind}`)}
        </span>
      )}
      {contact.blocked && (
        <span className="shrink-0 rounded-full bg-danger/10 px-2 text-12 font-medium text-danger" data-testid="contact-spam">
          {t("contacts.view.spamBadge")}
        </span>
      )}
    </>
  );
}

function usage(t: TFunction, sent: number, received: number, seen: number, lastUsed: number | null, language: string): string {
  return [
    t("contacts.sent", { count: sent }),
    t("contacts.received", { count: received }),
    ...(seen > 0 ? [t("contacts.view.seen", { count: seen })] : []),
    ...(lastUsed !== null ? [t("contacts.lastUsed", { date: new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(lastUsed) })] : []),
  ].join(" · ");
}

/** The picture (or the initials) with its buttons, or the cropper while the user cuts one. */
function PictureArea({ contact, name }: { contact: ContactEntry; name: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [source, setSource] = useState<ContactPictureSource | null>(null);
  const [loading, setLoading] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [error, setError] = useState<{ context: "choose" | "save" | "remove"; error: CoreError } | null>(null);
  const setPicture = useSetContactPicture();
  const removePicture = useRemoveContactPicture();
  const chooseButton = useRef<HTMLButtonElement>(null);

  const load = async (read: () => Promise<ContactPictureSource | null>) => {
    setLoading(true);
    setError(null);
    try {
      const chosen = await read();
      if (chosen) setSource(chosen);
    } catch (e) {
      setError({ context: "choose", error: toCoreError(e) });
    } finally {
      setLoading(false);
    }
  };

  // An image dropped on the window while this contact is open becomes its picture (the path comes
  // from the shell's own drop event; the core reads the file, the page never does).
  const onDrop = useEffectEvent((paths: string[]) => {
    const path = paths[0];
    // A composer open over the view takes the drop as an attachment.
    if (path === undefined || useComposerStore.getState().session !== null) return;
    void load(() => contactsIpc.loadDroppedContactPicture(path));
  });
  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void composeIpc
      .onFileDrop((event) => {
        if (event.type === "enter") setDropping(true);
        else if (event.type === "leave") setDropping(false);
        else if (event.type === "drop") {
          setDropping(false);
          if (event.paths.length > 0) onDrop(event.paths);
        }
      })
      .then((unsubscribe) => {
        if (active) stop = unsubscribe;
        else unsubscribe();
      });
    return () => {
      active = false;
      stop?.();
    };
  }, []);

  if (source) {
    return (
      <div className="flex flex-col gap-3">
        <PictureEditor
          source={source}
          name={name}
          saving={setPicture.isPending}
          onCancel={() => {
            setSource(null);
            setError(null);
          }}
          onSave={(crop) => {
            setError(null);
            setPicture.mutate(
              { email: contact.email, token: source.token, crop },
              {
                onSuccess: () => {
                  setSource(null);
                  refreshContacts(queryClient);
                },
                onError: (e) => {
                  setError({ context: "save", error: toCoreError(e) });
                },
              },
            );
          }}
        />
        {error && <ErrorNotice context={t("contacts.view.cropper.failed")} error={error.error} data-testid="picture-error" />}
      </div>
    );
  }
  return (
    <div className="flex shrink-0 flex-col items-center gap-2" data-testid="contact-picture-area">
      <div
        className={cx(
          "rounded-full p-1 transition-colors",
          dropping ? "bg-accent-soft outline-2 outline-focus-ring" : "bg-transparent",
        )}
      >
        <Avatar name={contact.name} email={contact.email} accountColor={0} ring={false} size="large" />
      </div>
      <div className="flex flex-col items-center gap-1">
        <button
          ref={chooseButton}
          type="button"
          disabled={loading}
          onClick={() => void load(() => contactsIpc.pickContactPicture())}
          className="inline-flex h-8 items-center gap-1 rounded-control px-2 text-12 font-medium text-accent hover:bg-hover disabled:opacity-50"
          data-testid="contact-picture-choose"
        >
          <ImagePlus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {loading
            ? t("contacts.view.picture.loading")
            : contact.picture !== null
              ? t("contacts.view.picture.change")
              : t("contacts.view.picture.choose")}
        </button>
        {contact.picture !== null && (
          <button
            type="button"
            disabled={removePicture.isPending}
            onClick={() => {
              setError(null);
              removePicture.mutate(contact.email, {
                onError: (e) => {
                  setError({ context: "remove", error: toCoreError(e) });
                },
              });
            }}
            className="inline-flex h-8 items-center gap-1 rounded-control px-2 text-12 text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
            data-testid="contact-picture-remove"
          >
            <Trash2 aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("contacts.view.picture.remove")}
          </button>
        )}
        <p className="max-w-(--ui-avatar-large) text-center text-12 text-tertiary">
          {dropping ? t("contacts.view.picture.dropping") : t("contacts.view.picture.dropHint")}
        </p>
      </div>
      {error && (
        <ErrorNotice
          context={error.context === "remove" ? t("contacts.view.picture.removeFailed") : t("contacts.view.picture.failed")}
          error={error.error}
          data-testid="picture-error"
        />
      )}
    </div>
  );
}

function NoteEditor({ contact }: { contact: ContactEntry }) {
  const { t } = useTranslation();
  const id = useId();
  const hintId = useId();
  const saved = contact.note ?? "";
  const [note, setNote] = useState(saved);
  const [shownSaved, setShownSaved] = useState(saved);
  const save = useSetContactNote();
  // Another window or device changed it while nothing was typed here.
  if (saved !== shownSaved) {
    setShownSaved(saved);
    if (note === shownSaved) setNote(saved);
  }
  const changed = note.trim() !== saved.trim();
  return (
    <div className="flex flex-col gap-2">
      <label htmlFor={id} className="sr-only">
        {t("contacts.view.note")}
      </label>
      <textarea
        id={id}
        value={note}
        rows={3}
        maxLength={1000}
        placeholder={t("contacts.view.notePlaceholder")}
        onChange={(event) => {
          setNote(event.target.value);
        }}
        aria-describedby={hintId}
        className="w-full resize-y rounded-control border border-border-strong bg-surface-raised px-3 py-2 text-14 text-primary placeholder:text-tertiary"
        data-testid="contact-note"
      />
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="secondary"
          disabled={!changed || save.isPending}
          onClick={() => {
            save.mutate({ email: contact.email, note: note.trim() === "" ? null : note });
          }}
          data-testid="contact-note-save"
        >
          {t("contacts.view.saveNote")}
        </Button>
        <p id={hintId} className="text-12 text-tertiary">
          {save.isSuccess && !changed ? t("contacts.view.noteSaved") : t("contacts.view.noteHint")}
        </p>
      </div>
      {save.isError && <ErrorNotice context={t("contacts.view.noteFailed")} error={save.error} data-testid="contact-note-error" />}
    </div>
  );
}

function RecentMail({ contact }: { contact: ContactEntry }) {
  const { t, i18n } = useTranslation();
  const messages = useContactMessages(contact.email);
  const bounds = dayBounds(new Date(), 1);
  if (messages.isPending) {
    return (
      <p role="status" className="text-13 text-secondary">
        {t("contacts.view.recentLoading")}
      </p>
    );
  }
  if (messages.isError) {
    return <ErrorNotice context={t("contacts.view.recentFailed")} error={messages.error} onRetry={() => void messages.refetch()} />;
  }
  const items = messages.data.items;
  if (items.length === 0) {
    return (
      <p className="text-13 text-secondary" data-testid="contact-recent-empty">
        {t("contacts.view.recentEmpty")}
      </p>
    );
  }
  const subjectOf = (item: MessageListItem) => (item.subject.trim() ? item.subject : t("list.noSubject"));
  return (
    <ul aria-label={t("contacts.view.recent")} className="flex flex-col overflow-hidden rounded-card border border-border-subtle bg-surface-raised">
      {items.map((item) => (
        <li key={item.id} className="border-b border-border-subtle last:border-b-0">
          <button
            type="button"
            onClick={() => {
              openMessageInWindow(item.accountId, item.id);
            }}
            className="flex w-full min-w-0 items-center gap-3 px-3 py-2 text-left hover:bg-hover"
            data-testid="contact-recent-message"
          >
            <span className="min-w-0 flex-1">
              <span className={cx("block truncate text-13", item.unread ? "font-semibold text-primary" : "text-primary")}>
                {subjectOf(item)}
              </span>
              {item.preview && <span className="block truncate text-12 text-tertiary">{item.preview}</span>}
            </span>
            <time
              dateTime={new Date(item.date).toISOString()}
              title={formatFullDate(item.date, i18n.language)}
              className="shrink-0 text-12 text-tertiary tabular-nums"
            >
              {formatListDate(item.date, bounds, i18n.language)}
            </time>
          </button>
        </li>
      ))}
    </ul>
  );
}

interface ContactDetailProps {
  entry: ContactEntry;
  /** The heading takes the focus (Enter in the list). */
  focusRequest: number;
}

/**
 * Stage 17 (decision 0050): one contact. Picture (chosen from a file or dropped, cut to a square,
 * or the initials), name, every address with its use, the user's note, the newest mail with it,
 * and "Write mail", "Show all mail" and "Mark as spam".
 */
export function ContactDetail({ entry, focusRequest }: ContactDetailProps) {
  const { t, i18n } = useTranslation();
  const headingId = useId();
  const heading = useRef<HTMLHeadingElement>(null);
  const contact = useContact(entry.email).data ?? entry;
  const name = contactName(contact, contact.email);
  const [editingName, setEditingName] = useState(false);
  const separate = useSeparateContact();
  const spam = useContactSpam();
  const { account } = useSelectedAccount();
  const showMailView = useUiStore((state) => state.showMailView);

  useEffect(() => {
    if (focusRequest > 0) heading.current?.focus();
  }, [focusRequest]);

  const writeMail = () => {
    if (!account) return;
    openComposer({ kind: "new", accountId: account.id, to: [{ name: contact.name, email: contact.email }] });
  };
  const showAllMail = () => {
    const search = useSearchStore.getState();
    showMailView("inbox");
    search.setScope("all");
    search.setQuery(`from:${contact.email}`);
    search.openSearch();
    search.submit();
  };

  return (
    <article aria-labelledby={headingId} className="mx-auto flex w-full max-w-(--ui-content-max-width) flex-col gap-8 px-8 py-8" data-testid="contact-detail">
      <header className="flex flex-wrap items-start gap-6">
        <PictureArea key={contact.key} contact={contact} name={name} />
        <div className="flex min-w-0 flex-1 flex-col gap-1 pt-2">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <h2 id={headingId} ref={heading} tabIndex={-1} className="min-w-0 truncate text-20 font-semibold text-primary" data-testid="contact-detail-name">
              {name}
            </h2>
            <ContactBadges contact={contact} />
          </div>
          <p className="truncate text-14 text-secondary" data-testid="contact-detail-email">
            {contact.email}
          </p>
          {contact.kind !== "person" && <p className="text-13 text-tertiary">{t(`contacts.view.kindHint.${contact.kind}`)}</p>}
          {contact.userName !== null && <p className="text-12 text-tertiary">{t("contacts.view.ownNameHint")}</p>}
        </div>
      </header>

      <div role="group" aria-label={t("contacts.view.actions", { name })} className="flex flex-wrap gap-2">
        <Button disabled={!account} onClick={writeMail} data-testid="contact-write">
          <SquarePen aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("contacts.view.write")}
        </Button>
        <Button variant="secondary" onClick={showAllMail} data-testid="contact-show-all">
          <Search aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("contacts.view.showAll")}
        </Button>
        {contact.blocked ? (
          <Button variant="secondary" disabled={spam.busy} onClick={() => void spam.notSpam(contact, name)} data-testid="contact-not-spam">
            <ShieldCheck aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("contacts.view.notSpam")}
          </Button>
        ) : (
          <Button variant="secondary" disabled={spam.busy} onClick={() => void spam.mark(contact, name)} data-testid="contact-mark-spam">
            <ShieldAlert aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("contacts.view.markSpam")}
          </Button>
        )}
        <Button
          variant="quiet"
          onClick={() => {
            setEditingName(true);
          }}
          aria-label={t("contacts.rename.openLabel", { email: contact.email })}
          data-testid="contact-rename"
        >
          <UserPen aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("contacts.rename.open")}
        </Button>
      </div>
      {spam.error && <ErrorNotice context={t("contacts.view.spam.failed")} error={spam.error} data-testid="contact-spam-error" />}
      {editingName && (
        <NameEditor
          contact={contact}
          onDone={() => {
            setEditingName(false);
          }}
        />
      )}

      <Section title={t("contacts.view.addresses")} data-testid="contact-addresses">
        <ul aria-label={t("contacts.view.addressesOf", { name })} className="flex flex-col overflow-hidden rounded-card border border-border-subtle bg-surface-raised">
          {contact.addresses.map((address, index) => (
            <li key={address.email} className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border-subtle px-3 py-2 last:border-b-0" data-testid="contact-address">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate text-14 text-primary">{address.email}</span>
                  {address.blocked && <span className="shrink-0 text-12 font-medium text-danger">{t("contacts.view.spamBadge")}</span>}
                </span>
                <span className="text-12 text-tertiary tabular-nums" data-testid="contact-usage">
                  {usage(t, address.sent, address.received, address.seen, address.lastUsed, i18n.language)}
                </span>
              </div>
              {index > 0 && (
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
              )}
            </li>
          ))}
        </ul>
        {separate.isError && <ErrorNotice context={t("contacts.separate.failed")} error={separate.error} data-testid="contact-separate-error" />}
      </Section>

      <Section title={t("contacts.view.note")} data-testid="contact-note-section">
        <NoteEditor key={contact.key} contact={contact} />
      </Section>

      <Section title={t("contacts.view.recent")} data-testid="contact-recent">
        <RecentMail contact={contact} />
      </Section>

      {spam.pending && (
        <ConfirmDialog
          tone="neutral"
          title={t("contacts.view.spam.moveTitle", { count: spam.pending.movable })}
          body={[
            t("contacts.view.spam.moveBody", { name: spam.pending.name }),
            ...(spam.pending.kept > 0 ? [t("contacts.view.spam.readOnly", { count: spam.pending.kept })] : []),
          ].join(" ")}
          confirmLabel={t("contacts.view.spam.move")}
          cancelLabel={t("contacts.view.spam.leave")}
          onConfirm={() => void spam.answer(true)}
          onCancel={() => void spam.answer(false)}
          data-testid="contact-spam-dialog"
        />
      )}
    </article>
  );
}
