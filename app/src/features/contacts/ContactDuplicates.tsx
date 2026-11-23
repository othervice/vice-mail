import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { ContactDuplicate } from "../../services/ipc";
import { contactName } from "./names";
import { useContactDuplicates, useDismissDuplicate, useMergeContacts, useSetContactName } from "./queries";

/** One suggestion: two contacts that may be one person, or one contact seen with several names. */
function DuplicateCard({ duplicate }: { duplicate: ContactDuplicate }) {
  const { t } = useTranslation();
  const legendId = useId();
  const [first] = duplicate.contacts;
  const [keep, setKeep] = useState(first?.key ?? "");
  const [confirming, setConfirming] = useState(false);
  const merge = useMergeContacts();
  const dismiss = useDismissDuplicate();
  const rename = useSetContactName();
  const busy = merge.isPending || dismiss.isPending || rename.isPending;
  const kept = duplicate.contacts.find((c) => c.key === keep) ?? first;
  const others = duplicate.contacts.filter((c) => c !== kept);
  const failed = merge.error ?? dismiss.error ?? rename.error;
  const reason =
    duplicate.reason === "same_name"
      ? t("contacts.duplicates.reason.same_name", { name: duplicate.name ?? "" })
      : t(`contacts.duplicates.reason.${duplicate.reason}`);

  return (
    <li
      className="flex flex-col gap-3 rounded-card border border-border p-4"
      data-testid="contact-duplicate"
      data-reason={duplicate.reason}
    >
      <p className="text-13 text-primary" data-testid="contact-duplicate-reason">
        {reason}
      </p>
      {duplicate.reason === "names_differ" && first ? (
        <>
          <p className="text-13 text-secondary">
            {t("contacts.duplicates.names", { email: first.email, names: duplicate.names.join(", ") })}
          </p>
          <div className="flex flex-wrap gap-2">
            {duplicate.names.map((name) => (
              <Button
                key={name}
                variant="secondary"
                disabled={busy}
                onClick={() => {
                  rename.mutate({ email: first.email, name });
                }}
                data-testid="contact-duplicate-use-name"
              >
                {t("contacts.duplicates.useName", { name })}
              </Button>
            ))}
            <Button
              variant="quiet"
              disabled={busy}
              onClick={() => {
                dismiss.mutate([first.key]);
              }}
              data-testid="contact-duplicate-dismiss"
            >
              {t("contacts.duplicates.keepAsIs")}
            </Button>
          </div>
        </>
      ) : (
        <>
          <fieldset aria-labelledby={legendId} className="flex flex-col gap-1">
            <legend id={legendId} className="text-12 font-medium text-secondary">
              {t("contacts.duplicates.keep")}
            </legend>
            {duplicate.contacts.map((contact) => (
              <label key={contact.key} className="flex items-center gap-2 text-13 text-primary">
                <input
                  type="radio"
                  name={`${legendId}-keep`}
                  value={contact.key}
                  checked={keep === contact.key}
                  onChange={() => {
                    setKeep(contact.key);
                  }}
                  data-testid="contact-duplicate-keep"
                />
                <span className="min-w-0 truncate">
                  {t("contacts.duplicates.option", { name: contactName(contact, t("contacts.noName")), email: contact.email })}
                </span>
              </label>
            ))}
          </fieldset>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              disabled={busy || !kept || others.length === 0}
              onClick={() => {
                setConfirming(true);
              }}
              data-testid="contact-duplicate-merge"
            >
              {t("contacts.duplicates.merge")}
            </Button>
            <Button
              variant="quiet"
              disabled={busy}
              onClick={() => {
                dismiss.mutate(duplicate.contacts.map((c) => c.key));
              }}
              data-testid="contact-duplicate-dismiss"
            >
              {t("contacts.duplicates.notSame")}
            </Button>
          </div>
        </>
      )}
      {failed && <ErrorNotice context={t("contacts.duplicates.failed")} error={failed} data-testid="contact-duplicate-error" />}
      {confirming && kept && (
        <ConfirmDialog
          title={t("contacts.duplicates.confirmTitle")}
          body={t("contacts.duplicates.confirmBody", { kept: kept.email, merged: others.map((c) => c.email).join(", ") })}
          confirmLabel={t("contacts.duplicates.confirm")}
          cancelLabel={t("contacts.duplicates.cancel")}
          onConfirm={() => {
            setConfirming(false);
            // The user confirmed in the dialog: only now the core may merge.
            merge.mutate({ keep: kept.email, merge: others.map((c) => c.email), confirmed: true });
          }}
          onCancel={() => {
            setConfirming(false);
          }}
          data-testid="contact-merge-confirm"
        />
      )}
    </li>
  );
}

/** Suggestions of possible duplicates; nothing is merged without the user's confirmation. */
export function ContactDuplicates() {
  const { t } = useTranslation();
  const headingId = useId();
  const duplicates = useContactDuplicates();
  if (duplicates.isError) {
    return (
      <ErrorNotice
        context={t("contacts.duplicates.loadFailed")}
        error={duplicates.error}
        onRetry={() => void duplicates.refetch()}
        data-testid="contact-duplicates-error"
      />
    );
  }
  const list = duplicates.data ?? [];
  if (list.length === 0) return null;
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3" data-testid="contact-duplicates">
      <div className="flex flex-col gap-1">
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {t("contacts.duplicates.title", { count: list.length })}
        </h3>
        <p className="text-12 text-secondary">{t("contacts.duplicates.hint")}</p>
      </div>
      <ul className="flex flex-col gap-3">
        {list.map((duplicate) => (
          <DuplicateCard key={duplicate.contacts.map((c) => c.key).join(" ")} duplicate={duplicate} />
        ))}
      </ul>
    </section>
  );
}
