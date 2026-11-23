import { FolderPlus, LoaderCircle } from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ComboBox } from "../../components/ComboBox";
import { ErrorNotice } from "../../components/ErrorNotice";
import { TextField } from "../../components/TextField";
import type { Account, CoreError, Mailbox } from "../../services/ipc";
import { useFolderOptions } from "./Pickers";
import type { MissingMailbox } from "./useActionController";

/**
 * Where a new top-level folder goes: next to the others, or below the inbox on servers that keep
 * every folder there ("INBOX.Archive").
 */
export function newFolderPath(mailboxes: readonly Mailbox[], name: string): string {
  const inbox = mailboxes.find((mailbox) => mailbox.role === "inbox");
  const delimiter = inbox?.delimiter;
  const others = mailboxes.filter((mailbox) => mailbox.role !== "inbox");
  if (inbox && delimiter && others.length > 0 && others.every((m) => m.path.startsWith(inbox.path + delimiter))) {
    return `${inbox.path}${delimiter}${name}`;
  }
  return name;
}

interface MissingMailboxPromptProps {
  account: Account;
  mailboxes: readonly Mailbox[];
  missing: MissingMailbox;
  onChoose: (mailbox: Mailbox) => Promise<CoreError | null>;
  onCreate: (path: string) => Promise<CoreError | null>;
  onCancel: () => void;
}

/**
 * `missing_mailbox`, asked in place above the list (no dialog for everyday things): which of the
 * account's folders is the archive (or trash, or junk folder), or create one. The action that
 * asked runs as soon as the folder is set; a created folder first has to exist on the server.
 */
export function MissingMailboxPrompt({ account, mailboxes, missing, onChoose, onCreate, onCancel }: MissingMailboxPromptProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const regionRef = useRef<HTMLElement>(null);
  const [query, setQuery] = useState("");
  const [name, setName] = useState<string>(() => t(`actions.missing.names.${missing.role === "archive" || missing.role === "trash" || missing.role === "junk" ? missing.role : "inbox"}`));
  const [nameError, setNameError] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);
  const [busy, setBusy] = useState(false);
  const role = t(`roles.${missing.role}`);
  const options = useFolderOptions(mailboxes, query, (mailbox) => mailbox.role !== "custom");

  useEffect(() => {
    regionRef.current?.focus();
  }, []);

  const run = async (work: () => Promise<CoreError | null>) => {
    setBusy(true);
    setError(null);
    const failure = await work();
    setBusy(false);
    if (failure) setError(failure);
  };

  const create = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) {
      setNameError(true);
      return;
    }
    void run(() => onCreate(newFolderPath(mailboxes, trimmed)));
  };

  return (
    <section
      ref={regionRef}
      tabIndex={-1}
      aria-labelledby={titleId}
      className="mx-2 mt-2 flex flex-col gap-4 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4"
      data-testid="missing-mailbox"
    >
      <div className="flex flex-col gap-1">
        <h2 id={titleId} className="text-14 font-semibold text-primary">
          {t("actions.missing.title", { role })}
        </h2>
        <p className="text-13 text-secondary">
          {t("actions.missing.body", { role, account: account.displayName || account.email })}
        </p>
      </div>
      {missing.creating !== null ? (
        <p role="status" className="flex items-center gap-2 text-13 text-secondary" data-testid="missing-creating">
          <LoaderCircle aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
          {t("actions.missing.creating", { name: missing.creating })}
        </p>
      ) : (
        <>
          {options.length > 0 || query ? (
            <ComboBox
              label={t("actions.missing.choose")}
              query={query}
              onQueryChange={setQuery}
              options={options}
              onChoose={(option) => {
                if (!busy) void run(() => onChoose(option.value));
              }}
              emptyText={t("actions.picker.empty", { query: query.trim() })}
              data-testid="missing-chooser"
            />
          ) : null}
          <form noValidate onSubmit={create} className="flex flex-col gap-2" data-testid="missing-create">
            <TextField
              label={t("actions.missing.name")}
              value={name}
              onChange={(next) => {
                setName(next);
                setNameError(false);
              }}
              error={nameError ? t("actions.missing.nameRequired") : null}
              data-testid="missing-name"
            />
            <div>
              <Button type="submit" variant="secondary" disabled={busy}>
                <FolderPlus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
                {t("actions.missing.createButton")}
              </Button>
            </div>
          </form>
        </>
      )}
      {error && <ErrorNotice context={t("actions.missing.failed")} error={error} />}
      <div>
        <Button variant="quiet" onClick={onCancel}>
          {t("actions.missing.cancel")}
        </Button>
      </div>
    </section>
  );
}
