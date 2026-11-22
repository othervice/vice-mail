import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../../components/ErrorNotice";
import type { Account, Mailbox, MailboxRole } from "../../../services/ipc";
import { useMailboxes, useSetMailboxRole } from "../../mailboxes/queries";
import { ASSIGNABLE_ROLES, mailboxWithRole } from "../../mailboxes/tree";

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

interface RoleRowProps {
  role: Exclude<MailboxRole, "inbox" | "custom">;
  mailboxes: readonly Mailbox[];
  choices: readonly Mailbox[];
  disabled: boolean;
  onChange: (mailbox: Mailbox | null, previous: Mailbox | undefined) => void;
}

function RoleRow({ role, mailboxes, choices, disabled, onChange }: RoleRowProps) {
  const { t } = useTranslation();
  const id = useId();
  const current = mailboxWithRole(mailboxes, role);
  const source = current && current.roleSource !== "none" ? t(`settings.accounts.detail.folders.source.${current.roleSource}`) : null;
  return (
    <div className="grid grid-cols-[minmax(0,10rem)_minmax(0,1fr)] items-center gap-x-4 gap-y-1">
      <label htmlFor={id} className="text-13 font-medium text-primary">
        {t(`roles.${role}`)}
      </label>
      <select
        id={id}
        value={current?.id ?? ""}
        disabled={disabled}
        aria-describedby={source ? `${id}-source` : undefined}
        onChange={(event) => {
          const value = event.target.value;
          onChange(value === "" ? null : (choices.find((m) => String(m.id) === value) ?? null), current);
        }}
        data-testid={`role-${role}`}
        className="h-8 min-w-0 rounded-control border border-border-strong bg-surface-raised px-2 text-14 text-primary disabled:opacity-50"
      >
        <option value="">{t("settings.accounts.detail.folders.none")}</option>
        {choices.map((mailbox) => (
          <option key={mailbox.id} value={mailbox.id}>
            {mailbox.path}
          </option>
        ))}
      </select>
      {source && (
        <p id={`${id}-source`} className="col-start-2 text-12 text-secondary">
          {source}
        </p>
      )}
    </div>
  );
}

/**
 * Which folder has which role (Sent, Drafts, Archive, …). The app detects them (SPECIAL-USE or
 * the folder name); here the user corrects them. Native selects: keyboard and screen reader
 * support of the system, and no injected styles (CSP).
 */
export function SpecialFolders({ account }: { account: Account }) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(account.id);
  const setRole = useSetMailboxRole(account.id);
  const choices = useMemo(
    () =>
      (mailboxes.data ?? [])
        .filter((m) => m.selectable && m.role !== "inbox")
        .sort((a, b) => collator.compare(a.path, b.path)),
    [mailboxes.data],
  );

  if (mailboxes.isPending) return null;
  if (mailboxes.isError) {
    return (
      <ErrorNotice
        context={t("settings.accounts.detail.folders.loadFailed")}
        error={mailboxes.error}
        onRetry={() => {
          void mailboxes.refetch();
        }}
      />
    );
  }
  if (choices.length === 0) return <p className="text-13 text-secondary">{t("settings.accounts.detail.folders.empty")}</p>;

  return (
    <div className="flex flex-col gap-3" data-testid="special-folders">
      {ASSIGNABLE_ROLES.map((role) => (
        <RoleRow
          key={role}
          role={role}
          mailboxes={mailboxes.data}
          choices={choices}
          disabled={setRole.isPending}
          onChange={(mailbox, previous) => {
            if (mailbox) setRole.mutate({ mailboxId: mailbox.id, role });
            // "None": the folder that had the role becomes an ordinary one.
            else if (previous) setRole.mutate({ mailboxId: previous.id, role: "custom" });
          }}
        />
      ))}
      {setRole.isError && <ErrorNotice context={t("settings.accounts.detail.folders.failed")} error={setRole.error} />}
    </div>
  );
}
