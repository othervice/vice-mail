import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { TextField } from "../../components/TextField";
import type { Account, AccountGroup, AccountGroupId } from "../../services/ipc";
import { accountFill } from "../accounts/accountColor";
import { cx } from "../../components/cx";
import { useAccountGroups, useDeleteAccountGroup, useSaveAccountGroup } from "./queries";

/** Longest group name the core accepts (trimmed). */
export const MAX_GROUP_NAME = 100;

interface GroupFormProps {
  accounts: readonly Account[];
  group: AccountGroup | null;
  onDone: () => void;
}

/** Name and members of a new or existing group; the core replaces the members on save. */
function GroupForm({ accounts, group, onDone }: GroupFormProps) {
  const { t } = useTranslation();
  const save = useSaveAccountGroup();
  const membersId = useId();
  const [name, setName] = useState(group?.name ?? "");
  const [members, setMembers] = useState<ReadonlySet<string>>(new Set(group?.accountIds ?? []));
  const [touched, setTouched] = useState(false);
  const nameMissing = !name.trim();
  const noMembers = members.size === 0;
  return (
    <form
      // Own messages next to the fields instead of the browser's validation bubbles.
      noValidate
      className="flex flex-col gap-3 rounded-card border border-border-subtle bg-surface-raised shadow-card p-4"
      onSubmit={(event) => {
        event.preventDefault();
        setTouched(true);
        if (nameMissing || noMembers) return;
        save.mutate(
          {
            groupId: group?.id ?? null,
            name: name.trim().slice(0, MAX_GROUP_NAME),
            // Account order, as the core keeps it.
            accountIds: accounts.filter((a) => members.has(a.id)).map((a) => a.id),
          },
          { onSuccess: onDone },
        );
      }}
      data-testid="group-form"
    >
      <TextField
        label={t("groups.name")}
        value={name}
        onChange={setName}
        required
        error={touched && nameMissing ? t("groups.nameRequired") : null}
        data-testid="group-name"
      />
      <fieldset className="flex flex-col gap-2" aria-describedby={touched && noMembers ? membersId : undefined}>
        <legend className="text-13 font-medium text-primary">{t("groups.members")}</legend>
        {accounts.map((account) => (
          <label key={account.id} className="flex items-center gap-2 text-14 text-primary">
            <input
              type="checkbox"
              checked={members.has(account.id)}
              onChange={(event) => {
                const next = new Set(members);
                if (event.target.checked) next.add(account.id);
                else next.delete(account.id);
                setMembers(next);
              }}
              className="size-4 accent-accent"
            />
            <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", accountFill(account.color))} />
            <span className="truncate">{account.displayName || account.email}</span>
          </label>
        ))}
        {touched && noMembers && (
          <p id={membersId} className="text-12 text-danger">
            {t("groups.membersRequired")}
          </p>
        )}
      </fieldset>
      {save.isError && <ErrorNotice context={t("groups.saveFailed")} error={save.error} />}
      <div className="flex gap-2">
        <Button type="submit" disabled={save.isPending} data-testid="group-save">
          {t("groups.save")}
        </Button>
        <Button variant="secondary" onClick={onDone}>
          {t("groups.cancel")}
        </Button>
      </div>
    </form>
  );
}

/**
 * Settings → Accounts, stage 7: groups of accounts whose inboxes the unified inbox can show
 * together. Groups are synced like accounts (stage 11); hiding an account is per device.
 */
export function AccountGroupsSection({ accounts }: { accounts: readonly Account[] }) {
  const { t } = useTranslation();
  const headingId = useId();
  const groups = useAccountGroups();
  const remove = useDeleteAccountGroup();
  const [editing, setEditing] = useState<AccountGroupId | null>(null);
  const [adding, setAdding] = useState(false);
  const names = new Map(accounts.map((a) => [a.id, a.displayName || a.email] as const));
  if (accounts.length === 0) return null;

  return (
    <section aria-labelledby={headingId} className="flex max-w-(--ui-content-max-width) flex-col gap-3" data-testid="account-groups">
      <div className="flex flex-col gap-1">
        <h3 id={headingId} className="text-14 font-semibold text-primary">
          {t("groups.title")}
        </h3>
        <p className="text-13 text-secondary">{t("groups.description")}</p>
      </div>
      {groups.isError && (
        <ErrorNotice
          context={t("groups.loadFailed")}
          error={groups.error}
          onRetry={() => {
            void groups.refetch();
          }}
        />
      )}
      {groups.isSuccess && groups.data.length === 0 && !adding && (
        <p className="text-13 text-secondary">{t("groups.empty")}</p>
      )}
      {groups.isSuccess && groups.data.length > 0 && (
        <ul aria-label={t("groups.title")} className="flex flex-col gap-2">
          {groups.data.map((group) =>
            editing === group.id ? (
              <li key={group.id}>
                <GroupForm
                  accounts={accounts}
                  group={group}
                  onDone={() => {
                    setEditing(null);
                  }}
                />
              </li>
            ) : (
              <li key={group.id} className="flex flex-wrap items-center gap-3 rounded-card border border-border px-3 py-2" data-testid="group">
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-14 font-medium text-primary">{group.name}</span>
                  <span className="truncate text-12 text-secondary">
                    {group.accountIds.map((id) => names.get(id) ?? id).join(", ") ||
                      t("groups.memberCount", { count: group.accountIds.length })}
                  </span>
                </div>
                <Button
                  variant="secondary"
                  aria-label={t("groups.edit", { name: group.name })}
                  onClick={() => {
                    setEditing(group.id);
                  }}
                >
                  {t("identities.edit")}
                </Button>
                <Button
                  variant="secondary"
                  disabled={remove.isPending}
                  aria-label={t("groups.delete", { name: group.name })}
                  onClick={() => {
                    remove.mutate(group.id);
                  }}
                >
                  {t("identities.delete")}
                </Button>
              </li>
            ),
          )}
        </ul>
      )}
      {remove.isError && <ErrorNotice context={t("groups.deleteFailed")} error={remove.error} />}
      {adding ? (
        <GroupForm
          accounts={accounts}
          group={null}
          onDone={() => {
            setAdding(false);
          }}
        />
      ) : (
        <div>
          <Button
            variant="secondary"
            onClick={() => {
              setEditing(null);
              setAdding(true);
            }}
            data-testid="group-add"
          >
            {t("groups.add")}
          </Button>
        </div>
      )}
    </section>
  );
}
