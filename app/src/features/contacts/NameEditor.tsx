import { useId, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { ContactEntry } from "../../services/ipc";
import { useSetContactName } from "./queries";

/** The name the user gives a contact (stage 10); empty goes back to the name seen in mail. */
export function NameEditor({ contact, onDone }: { contact: ContactEntry; onDone: () => void }) {
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
