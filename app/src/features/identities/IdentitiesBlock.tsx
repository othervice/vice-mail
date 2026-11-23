import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { TextField } from "../../components/TextField";
import type { Account, IdentityId, IdentityInfo } from "../../services/ipc";
import { useDeleteIdentity, useIdentities, useSaveIdentity, useSetDefaultIdentity } from "./queries";

interface FormProps {
  account: Account;
  /** The identity to change; null adds one. */
  identity: IdentityInfo | null;
  onDone: () => void;
}

/** Add or change an identity: address, name and an optional reply-to address. */
function IdentityForm({ account, identity, onDone }: FormProps) {
  const { t } = useTranslation();
  const save = useSaveIdentity(account.id);
  const [email, setEmail] = useState(identity?.email ?? "");
  const [name, setName] = useState(identity?.name ?? "");
  const [replyTo, setReplyTo] = useState(identity?.replyTo ?? "");
  const [touched, setTouched] = useState(false);
  const missing = !email.trim();
  return (
    <form
      // Own messages next to the fields instead of the browser's validation bubbles.
      noValidate
      className="flex flex-col gap-3 rounded-card border border-border bg-canvas p-4"
      onSubmit={(event) => {
        event.preventDefault();
        setTouched(true);
        if (missing) return;
        save.mutate(
          { identityId: identity?.id ?? null, identity: { email: email.trim(), name: name.trim(), replyTo: replyTo.trim() || null } },
          { onSuccess: onDone },
        );
      }}
      data-testid="identity-form"
    >
      <TextField
        label={t("identities.email")}
        type="email"
        value={email}
        onChange={setEmail}
        required
        error={touched && missing ? t("identities.emailRequired") : null}
        data-testid="identity-email"
      />
      <TextField label={t("identities.name")} value={name} onChange={setName} data-testid="identity-name" />
      <TextField label={t("identities.replyTo")} type="email" value={replyTo} onChange={setReplyTo} data-testid="identity-reply-to" />
      {save.isError && <ErrorNotice context={t("identities.saveFailed")} error={save.error} />}
      <div className="flex gap-2">
        <Button type="submit" disabled={save.isPending} data-testid="identity-save">
          {t("identities.save")}
        </Button>
        <Button variant="secondary" onClick={onDone}>
          {t("identities.cancel")}
        </Button>
      </div>
    </form>
  );
}

/**
 * Settings of one account, stage 7: its sender addresses (inside the page's "Identities" block). One is the default; the default
 * cannot be deleted (the core refuses it too), so its delete button explains why instead.
 */
export function IdentitiesBlock({ account }: { account: Account }) {
  const { t } = useTranslation();
  const noteId = useId();
  const identities = useIdentities(account.id);
  const setDefault = useSetDefaultIdentity(account.id);
  const remove = useDeleteIdentity(account.id);
  const [editing, setEditing] = useState<IdentityId | "new" | null>(null);
  const accountName = account.displayName || account.email;

  return (
    <div className="flex flex-col gap-3" data-testid="identities">
      {identities.isError && (
        <ErrorNotice
          context={t("identities.loadFailed")}
          error={identities.error}
          onRetry={() => {
            void identities.refetch();
          }}
        />
      )}
      {identities.isSuccess && identities.data.length === 0 && <p className="text-13 text-secondary">{t("identities.empty")}</p>}
      {identities.isSuccess && identities.data.length > 0 && (
        <ul aria-label={t("identities.listLabel", { account: accountName })} className="flex flex-col gap-2">
          {identities.data.map((identity) =>
            editing === identity.id ? (
              <li key={identity.id}>
                <IdentityForm
                  account={account}
                  identity={identity}
                  onDone={() => {
                    setEditing(null);
                  }}
                />
              </li>
            ) : (
              <li
                key={identity.id}
                className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-card border border-border px-3 py-2"
                data-testid="identity"
              >
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-14 text-primary select-text">
                    {identity.name ? t("nav.account", { name: identity.name, email: identity.email }) : identity.email}
                  </span>
                  {identity.replyTo && (
                    <span className="truncate text-12 text-secondary select-text">
                      {t("identities.replyToShown", { address: identity.replyTo })}
                    </span>
                  )}
                </div>
                {identity.isDefault ? (
                  <span className="shrink-0 rounded-control border border-border-strong px-2 text-12 font-medium text-secondary" data-testid="identity-default">
                    {t("identities.default")}
                  </span>
                ) : (
                  <Button
                    variant="quiet"
                    disabled={setDefault.isPending}
                    aria-label={t("identities.makeDefaultLabel", { email: identity.email })}
                    onClick={() => {
                      setDefault.mutate(identity.id);
                    }}
                  >
                    {t("identities.makeDefault")}
                  </Button>
                )}
                <Button
                  variant="secondary"
                  aria-label={t("identities.editLabel", { email: identity.email })}
                  onClick={() => {
                    setEditing(identity.id);
                  }}
                >
                  {t("identities.edit")}
                </Button>
                <Button
                  variant="secondary"
                  disabled={identity.isDefault || remove.isPending}
                  aria-label={t("identities.deleteLabel", { email: identity.email })}
                  {...(identity.isDefault && { "aria-describedby": noteId })}
                  onClick={() => {
                    remove.mutate(identity.id);
                  }}
                >
                  {t("identities.delete")}
                </Button>
              </li>
            ),
          )}
        </ul>
      )}
      {identities.data?.some((identity) => identity.isDefault) && (
        <p id={noteId} className="text-12 text-secondary">
          {t("identities.deleteDefault")}
        </p>
      )}
      {setDefault.isError && <ErrorNotice context={t("identities.defaultFailed")} error={setDefault.error} />}
      {remove.isError && <ErrorNotice context={t("identities.deleteFailed")} error={remove.error} />}
      {editing === "new" ? (
        <IdentityForm
          account={account}
          identity={null}
          onDone={() => {
            setEditing(null);
          }}
        />
      ) : (
        <div>
          <Button
            variant="secondary"
            onClick={() => {
              setEditing("new");
            }}
            data-testid="identity-add"
          >
            {t("identities.add")}
          </Button>
        </div>
      )}
    </div>
  );
}
