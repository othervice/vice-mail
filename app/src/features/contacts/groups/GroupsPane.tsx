import { Pencil, Plus, Trash2, UsersRound, X } from "lucide-react";
import { useId, useState, type FormEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/Button";
import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { cx } from "../../../components/cx";
import { EmptyState } from "../../../components/EmptyState";
import { ErrorNotice } from "../../../components/ErrorNotice";
import { IconButton } from "../../../components/IconButton";
import { MAX_CONTACT_LIST_NAME_CHARS, type Address, type ContactListId, type ContactListSummary } from "../../../services/ipc";
import { RecipientField } from "../../composer/RecipientField";
import {
  useAddContactListMembers,
  useContactList,
  useContactLists,
  useCreateContactList,
  useDeleteContactList,
  useRemoveContactListMember,
  useRenameContactList,
} from "./queries";

function optionId(listId: string, id: string): string {
  return `${listId}-${id}`;
}

interface GroupListProps {
  selected: ContactListId | null;
  onSelect: (id: ContactListId | null) => void;
}

/**
 * Stage 19 (decision 0056): the groups of the contacts page, by name, with "New group". The list
 * is a listbox like the contacts: arrow keys move, Enter goes to the group.
 */
export function GroupList({ selected, onSelect }: GroupListProps) {
  const { t } = useTranslation();
  const listId = useId();
  const nameId = useId();
  const lists = useContactLists();
  const create = useCreateContactList();
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const groups: ContactListSummary[] = lists.data ?? [];
  const current = groups.find((g) => g.id === selected) ?? groups[0];

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (name.trim() === "") return;
    create.mutate(
      { name, members: [] },
      {
        onSuccess: (list) => {
          setCreating(false);
          setName("");
          onSelect(list.id);
        },
      },
    );
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = Math.max(0, groups.findIndex((g) => g.id === current?.id));
    const moves: Record<string, number> = { ArrowDown: index + 1, ArrowUp: index - 1, Home: 0, End: groups.length - 1 };
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    const next = groups[Math.max(0, Math.min(groups.length - 1, target))];
    if (next) onSelect(next.id);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="contact-groups">
      <div className="flex flex-col gap-2 border-b border-border-subtle px-4 py-3">
        {creating ? (
          <form onSubmit={submit} className="flex flex-col gap-2" data-testid="contact-group-new-form">
            <label htmlFor={nameId} className="text-12 text-secondary">
              {t("contactGroups.nameLabel")}
            </label>
            <input
              id={nameId}
              value={name}
              maxLength={MAX_CONTACT_LIST_NAME_CHARS}
              autoFocus
              onChange={(e) => {
                setName(e.target.value);
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  setCreating(false);
                }
              }}
              className="h-8 w-full rounded-control border border-border bg-surface-raised px-3 text-14 text-primary"
              data-testid="contact-group-name-input"
            />
            <div className="flex gap-2">
              <Button type="submit" disabled={create.isPending || name.trim() === ""} data-testid="contact-group-create">
                {t("contactGroups.create")}
              </Button>
              <Button
                variant="quiet"
                onClick={() => {
                  setCreating(false);
                }}
              >
                {t("contactGroups.cancel")}
              </Button>
            </div>
            {create.isError && <ErrorNotice context={t("contactGroups.createFailed")} error={create.error} />}
          </form>
        ) : (
          <Button
            variant="secondary"
            onClick={() => {
              create.reset();
              setCreating(true);
            }}
            data-testid="contact-group-new"
          >
            <Plus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("contactGroups.new")}
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {lists.isError ? (
          <div className="px-4 py-3">
            <ErrorNotice context={t("contactGroups.loadFailed")} error={lists.error} onRetry={() => void lists.refetch()} />
          </div>
        ) : groups.length === 0 ? (
          lists.isPending ? null : (
            <p className="px-4 py-6 text-center text-13 text-secondary" data-testid="contact-groups-empty">
              {t("contactGroups.empty")}
            </p>
          )
        ) : (
          <div
            id={listId}
            role="listbox"
            tabIndex={0}
            aria-label={t("contactGroups.list")}
            aria-activedescendant={current ? optionId(listId, current.id) : undefined}
            onKeyDown={onKeyDown}
            className="flex flex-col py-1 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus-ring"
            data-testid="contact-groups-list"
          >
            {groups.map((group) => (
              <div
                key={group.id}
                id={optionId(listId, group.id)}
                role="option"
                aria-selected={group.id === current?.id}
                onClick={() => {
                  onSelect(group.id);
                }}
                className={cx(
                  "mx-2 flex cursor-default items-center gap-3 rounded-tile px-2 py-2",
                  group.id === current?.id ? "bg-selected" : "hover:bg-hover",
                )}
                data-testid="contact-group-row"
              >
                <UsersRound aria-hidden className="size-(--ui-icon) shrink-0 text-secondary" strokeWidth={1.75} />
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-14 font-medium text-primary">{group.name}</span>
                  <span className="truncate text-12 text-secondary">{t("contactGroups.members", { count: group.memberCount })}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/** The group shown on the right: its name, its members, and adding or removing them. */
export function GroupDetail({ id, onDeleted }: { id: ContactListId; onDeleted: () => void }) {
  const { t } = useTranslation();
  const titleId = useId();
  const nameId = useId();
  const list = useContactList(id);
  const rename = useRenameContactList();
  const remove = useDeleteContactList();
  const addMembers = useAddContactListMembers();
  const removeMember = useRemoveContactListMember();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [adding, setAdding] = useState<Address[]>([]);
  const [confirming, setConfirming] = useState(false);

  if (list.isError) {
    return (
      <div className="p-6">
        <ErrorNotice context={t("contactGroups.loadFailed")} error={list.error} onRetry={() => void list.refetch()} />
      </div>
    );
  }
  const group = list.data;
  if (!group) return null;

  const saveName = (event: FormEvent) => {
    event.preventDefault();
    rename.mutate(
      { id, name },
      {
        onSuccess: () => {
          setEditing(false);
        },
      },
    );
  };
  const error = rename.error ?? addMembers.error ?? removeMember.error ?? remove.error;

  return (
    <section aria-labelledby={titleId} className="flex flex-col gap-6 p-6" data-testid="contact-group-detail">
      <div className="flex items-start justify-between gap-4">
        {editing ? (
          <form onSubmit={saveName} className="flex min-w-0 flex-1 flex-col gap-2">
            <label htmlFor={nameId} className="text-12 text-secondary">
              {t("contactGroups.nameLabel")}
            </label>
            <div className="flex gap-2">
              <input
                id={nameId}
                value={name}
                maxLength={MAX_CONTACT_LIST_NAME_CHARS}
                autoFocus
                onChange={(e) => {
                  setName(e.target.value);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setEditing(false);
                  }
                }}
                className="h-8 min-w-0 flex-1 rounded-control border border-border bg-surface-raised px-3 text-14 text-primary"
                data-testid="contact-group-rename-input"
              />
              <Button type="submit" disabled={rename.isPending || name.trim() === ""} data-testid="contact-group-rename-save">
                {t("contactGroups.save")}
              </Button>
            </div>
          </form>
        ) : (
          <div className="flex min-w-0 items-center gap-2">
            <h2 id={titleId} className="truncate text-20 font-semibold text-primary select-text" data-testid="contact-group-title">
              {group.name}
            </h2>
            <IconButton
              icon={Pencil}
              label={t("contactGroups.rename")}
              onClick={() => {
                setName(group.name);
                setEditing(true);
              }}
            />
          </div>
        )}
        <Button
          variant="quiet"
          onClick={() => {
            setConfirming(true);
          }}
          data-testid="contact-group-delete"
        >
          <Trash2 aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
          {t("contactGroups.delete")}
        </Button>
      </div>
      {error && <ErrorNotice context={t("contactGroups.changeFailed")} error={error} />}
      <div className="flex flex-col gap-2">
        <h3 className="text-13 font-semibold text-primary">{t("contactGroups.members", { count: group.members.length })}</h3>
        {group.members.length === 0 ? (
          <p className="text-13 text-secondary">{t("contactGroups.noMembers")}</p>
        ) : (
          <ul aria-label={t("contactGroups.memberList")} className="flex flex-col" data-testid="contact-group-members">
            {group.members.map((member) => (
              <li key={member.email} className="flex items-center gap-3 rounded-tile px-2 py-1 hover:bg-hover" data-testid="contact-group-member">
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-13 text-primary">{member.name ?? member.email}</span>
                  {member.name && <span className="truncate text-12 text-secondary">{member.email}</span>}
                </div>
                <IconButton
                  icon={X}
                  label={t("contactGroups.removeMember", { email: member.email })}
                  onClick={() => {
                    removeMember.mutate({ id, email: member.email });
                  }}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="flex flex-col gap-2" data-testid="contact-group-add">
        <h3 className="text-13 font-semibold text-primary">{t("contactGroups.addTitle")}</h3>
        <div className="rounded-card border border-border-subtle bg-surface-raised">
          <RecipientField
            label={t("contactGroups.addLabel")}
            addresses={adding}
            onChange={setAdding}
            onBlur={() => undefined}
            refused={null}
            disabled={addMembers.isPending}
            data-testid="contact-group-add-field"
          />
        </div>
        <div>
          <Button
            disabled={adding.length === 0 || addMembers.isPending}
            onClick={() => {
              addMembers.mutate(
                { id, members: adding },
                {
                  onSuccess: () => {
                    setAdding([]);
                  },
                },
              );
            }}
            data-testid="contact-group-add-submit"
          >
            {t("contactGroups.add")}
          </Button>
        </div>
      </div>
      {confirming && (
        <ConfirmDialog
          title={t("contactGroups.deleteTitle", { name: group.name })}
          body={t("contactGroups.deleteBody")}
          confirmLabel={t("contactGroups.delete")}
          cancelLabel={t("contactGroups.cancel")}
          onCancel={() => {
            setConfirming(false);
          }}
          onConfirm={() => {
            setConfirming(false);
            remove.mutate(id, { onSuccess: onDeleted });
          }}
          data-testid="contact-group-delete-dialog"
        />
      )}
    </section>
  );
}

/** The right side without a group. */
export function NoGroup() {
  const { t } = useTranslation();
  return <EmptyState icon={UsersRound} title={t("contactGroups.noGroupTitle")} body={t("contactGroups.noGroupBody")} />;
}
