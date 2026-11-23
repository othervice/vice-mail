import { ArrowDown, ArrowUp, Pencil, Plus, Trash2, X } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ChoiceGroup } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { SettingsSection } from "../../components/SettingsSection";
import { Switch } from "../../components/Switch";
import { TextField } from "../../components/TextField";
import type { Account, CoreError, MessageCategory, RuleAction, RuleCondition, RuleId, RuleInput, TextMatchOp } from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
import { useMailboxes } from "../mailboxes/queries";
import { CATEGORIES } from "./CategoryPanel";
import {
  ACTION_TYPES,
  CONDITION_FIELDS,
  emptyRule,
  MAX_RULES,
  moved,
  newAction,
  newCondition,
  opsFor,
  ruleInputOf,
  ruleProblem,
  useDeleteRule,
  useReorderRules,
  useRules,
  useSaveRule,
  type ActionType,
  type ConditionField,
} from "./rules";

const SELECT =
  "min-w-0 rounded-control border border-border bg-surface px-2 py-1 text-14 text-primary focus-visible:outline-2 focus-visible:outline-focus-ring";

interface SelectProps<T extends string> {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  "data-testid"?: string;
}

/** A native select (the CSP rules out Radix Select), with its label for screen readers. */
function Select<T extends string>({ label, value, options, onChange, ...rest }: SelectProps<T>) {
  return (
    <select
      aria-label={label}
      value={value}
      onChange={(event) => {
        const option = options.find((o) => o.value === event.target.value);
        if (option) onChange(option.value);
      }}
      data-testid={rest["data-testid"]}
      className={SELECT}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

function accountName(account: Account): string {
  return account.displayName || account.email;
}

/**
 * Settings → Inbox → Local rules: conditions and actions that run on new mail in the inbox while
 * the app is running (labelled as local). Actions that change mail go through the outbox and
 * are skipped, with a notice, on read-only accounts.
 */
export function LocalRules() {
  const { t } = useTranslation();
  const rules = useRules();
  const accounts = useAccounts();
  const remove = useDeleteRule();
  const reorder = useReorderRules();
  const save = useSaveRule();
  const [editing, setEditing] = useState<{ id: RuleId | null; input: RuleInput } | null>(null);
  const list = rules.data ?? [];
  const ids = list.map((rule) => rule.id);

  return (
    <SettingsSection title={t("comfort.rules.title")}>
      <p className="text-13 text-secondary" data-testid="rules-local-note">
        {t("comfort.rules.local")}
      </p>
      {rules.isError && (
        <ErrorNotice
          context={t("comfort.rules.loadFailed")}
          error={rules.error}
          onRetry={() => {
            void rules.refetch();
          }}
        />
      )}
      {rules.data && list.length === 0 && !editing && (
        <p className="text-13 text-secondary" data-testid="rules-empty">
          {t("comfort.rules.empty")}
        </p>
      )}
      {list.length > 0 && (
        <ol className="flex flex-col divide-y divide-border rounded-card border border-border" data-testid="rules">
          {list.map((rule, index) => (
            <li key={rule.id} className="flex flex-col gap-2 px-4 py-3" data-testid="rule">
              <Switch
                label={rule.name}
                description={t("comfort.rules.summary", {
                  conditions: rule.conditions.length,
                  actions: rule.actions.length,
                })}
                checked={rule.enabled}
                disabled={save.isPending}
                onChange={(enabled) => {
                  save.mutate({ id: rule.id, input: { ...ruleInputOf(rule), enabled } });
                }}
              />
              <div className="flex items-center gap-1">
                <IconButton
                  icon={ArrowUp}
                  label={t("comfort.rules.moveUp", { name: rule.name })}
                  disabled={index === 0 || reorder.isPending}
                  onClick={() => {
                    reorder.mutate(moved(ids, index, -1));
                  }}
                />
                <IconButton
                  icon={ArrowDown}
                  label={t("comfort.rules.moveDown", { name: rule.name })}
                  disabled={index === list.length - 1 || reorder.isPending}
                  onClick={() => {
                    reorder.mutate(moved(ids, index, 1));
                  }}
                />
                <IconButton
                  icon={Pencil}
                  label={t("comfort.rules.edit", { name: rule.name })}
                  onClick={() => {
                    setEditing({ id: rule.id, input: ruleInputOf(rule) });
                  }}
                />
                <IconButton
                  icon={Trash2}
                  label={t("comfort.rules.delete", { name: rule.name })}
                  disabled={remove.isPending}
                  onClick={() => {
                    remove.mutate(rule.id);
                  }}
                />
              </div>
            </li>
          ))}
        </ol>
      )}
      {remove.isError && <ErrorNotice context={t("comfort.rules.deleteFailed")} error={remove.error} />}
      {reorder.isError && <ErrorNotice context={t("comfort.rules.reorderFailed")} error={reorder.error} />}
      {editing ? (
        <RuleEditor
          key={editing.id ?? "new"}
          initial={editing.input}
          accounts={accounts.data ?? []}
          saving={save.isPending}
          error={save.error}
          onCancel={() => {
            save.reset();
            setEditing(null);
          }}
          onSave={(input) => {
            save.mutate(
              { id: editing.id, input },
              {
                onSuccess: () => {
                  setEditing(null);
                },
              },
            );
          }}
        />
      ) : (
        <div>
          <Button
            variant="secondary"
            disabled={!rules.data || list.length >= MAX_RULES}
            onClick={() => {
              setEditing({ id: null, input: emptyRule() });
            }}
            data-testid="rule-new"
          >
            <Plus aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
            {t("comfort.rules.new")}
          </Button>
        </div>
      )}
    </SettingsSection>
  );
}

interface RuleEditorProps {
  initial: RuleInput;
  accounts: readonly Account[];
  saving: boolean;
  error: CoreError | null;
  onSave: (input: RuleInput) => void;
  onCancel: () => void;
}

function Part({ children, onRemove, removeLabel }: { children: ReactNode; onRemove: () => void; removeLabel: string }) {
  return (
    <li className="flex flex-wrap items-center gap-2">
      {children}
      <IconButton icon={X} label={removeLabel} onClick={onRemove} />
    </li>
  );
}

function RuleEditor({ initial, accounts, saving, error, onSave, onCancel }: RuleEditorProps) {
  const { t } = useTranslation();
  const headingId = useId();
  const [input, setInput] = useState<RuleInput>(initial);
  const [tried, setTried] = useState(false);
  const firstAccount = accounts[0]?.id ?? null;
  const problem = ruleProblem(input);
  const set = (patch: Partial<RuleInput>) => {
    setInput((current) => ({ ...current, ...patch }));
  };
  const setCondition = (index: number, condition: RuleCondition) => {
    set({ conditions: input.conditions.map((c, i) => (i === index ? condition : c)) });
  };
  const setAction = (index: number, action: RuleAction) => {
    set({ actions: input.actions.map((a, i) => (i === index ? action : a)) });
  };
  const fieldOptions = CONDITION_FIELDS.map((field) => ({ value: field, label: t(`comfort.rules.fields.${field}`) }));
  const typeOptions = ACTION_TYPES.map((type) => ({ value: type, label: t(`comfort.rules.actions.${type}`) }));
  const accountOptions = accounts.map((account) => ({ value: account.id, label: accountName(account) }));
  const categoryOptions = CATEGORIES.map((category) => ({ value: category, label: t(`comfort.categories.${category}`) }));

  return (
    <form
      aria-labelledby={headingId}
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        setTried(true);
        if (ruleProblem(input) === null) onSave({ ...input, name: input.name.trim() });
      }}
      className="flex flex-col gap-4 rounded-card border border-border bg-surface-raised p-4"
      data-testid="rule-editor"
    >
      <h3 id={headingId} className="text-14 font-semibold text-primary">
        {initial.name ? t("comfort.rules.editTitle", { name: initial.name }) : t("comfort.rules.newTitle")}
      </h3>
      <TextField
        label={t("comfort.rules.name")}
        value={input.name}
        onChange={(name) => {
          set({ name });
        }}
        error={tried && problem === "name" ? t("comfort.rules.problems.name") : null}
        data-testid="rule-name"
      />
      <ChoiceGroup
        label={t("comfort.rules.match.label")}
        value={input.matchAll ? "all" : "any"}
        options={[
          { value: "all", label: t("comfort.rules.match.all") },
          { value: "any", label: t("comfort.rules.match.any") },
        ]}
        onChange={(value) => {
          set({ matchAll: value === "all" });
        }}
      />
      <fieldset className="flex flex-col gap-2">
        <legend className="pb-1 text-13 font-medium text-primary">{t("comfort.rules.conditions")}</legend>
        {input.conditions.length === 0 && <p className="text-13 text-secondary">{t("comfort.rules.everyMessage")}</p>}
        <ul className="flex flex-col gap-2">
          {input.conditions.map((condition, index) => (
            <Part
              key={index}
              removeLabel={t("comfort.rules.removeCondition", { index: index + 1 })}
              onRemove={() => {
                set({ conditions: input.conditions.filter((_, i) => i !== index) });
              }}
            >
              <Select<ConditionField>
                label={t("comfort.rules.fieldLabel", { index: index + 1 })}
                value={condition.field}
                options={fieldOptions}
                onChange={(field) => {
                  setCondition(index, newCondition(field, firstAccount));
                }}
                data-testid="rule-condition-field"
              />
              {condition.field === "account" ? (
                <Select<string>
                  label={t("comfort.rules.accountLabel")}
                  value={condition.accountId}
                  options={accountOptions}
                  onChange={(accountId) => {
                    setCondition(index, { field: "account", accountId });
                  }}
                />
              ) : (
                <>
                  <Select<TextMatchOp>
                    label={t("comfort.rules.opLabel", { index: index + 1 })}
                    value={condition.op}
                    options={opsFor(condition.field).map((op) => ({ value: op, label: t(`comfort.rules.ops.${op}`) }))}
                    onChange={(op) => {
                      setCondition(index, { ...condition, op });
                    }}
                    data-testid="rule-condition-op"
                  />
                  <input
                    aria-label={t("comfort.rules.valueLabel", { index: index + 1 })}
                    value={condition.value}
                    maxLength={200}
                    onChange={(event) => {
                      setCondition(index, { ...condition, value: event.target.value });
                    }}
                    data-testid="rule-condition-value"
                    className={SELECT}
                  />
                </>
              )}
            </Part>
          ))}
        </ul>
        <div>
          <Button
            variant="quiet"
            disabled={input.conditions.length >= 20}
            onClick={() => {
              set({ conditions: [...input.conditions, newCondition("from", firstAccount)] });
            }}
            data-testid="rule-add-condition"
          >
            {t("comfort.rules.addCondition")}
          </Button>
        </div>
      </fieldset>
      <fieldset className="flex flex-col gap-2">
        <legend className="pb-1 text-13 font-medium text-primary">{t("comfort.rules.actionsLegend")}</legend>
        <ul className="flex flex-col gap-2">
          {input.actions.map((action, index) => (
            <Part
              key={index}
              removeLabel={t("comfort.rules.removeAction", { index: index + 1 })}
              onRemove={() => {
                set({ actions: input.actions.filter((_, i) => i !== index) });
              }}
            >
              <Select<ActionType>
                label={t("comfort.rules.actionLabel", { index: index + 1 })}
                value={action.type}
                options={typeOptions}
                onChange={(type) => {
                  setAction(index, newAction(type, firstAccount));
                }}
                data-testid="rule-action-type"
              />
              {action.type === "move" && (
                <MoveTarget
                  action={action}
                  accountOptions={accountOptions}
                  onChange={(next) => {
                    setAction(index, next);
                  }}
                />
              )}
              {action.type === "setCategory" && (
                <Select<MessageCategory>
                  label={t("comfort.rules.categoryLabel")}
                  value={action.category}
                  options={categoryOptions}
                  onChange={(category) => {
                    setAction(index, { type: "setCategory", category });
                  }}
                />
              )}
              {action.type === "notify" && (
                <Select<"on" | "off">
                  label={t("comfort.rules.notifyLabel")}
                  value={action.enabled ? "on" : "off"}
                  options={[
                    { value: "on", label: t("comfort.rules.notifyOn") },
                    { value: "off", label: t("comfort.rules.notifyOff") },
                  ]}
                  onChange={(value) => {
                    setAction(index, { type: "notify", enabled: value === "on" });
                  }}
                />
              )}
            </Part>
          ))}
        </ul>
        <div>
          <Button
            variant="quiet"
            disabled={input.actions.length >= 20}
            onClick={() => {
              set({ actions: [...input.actions, newAction("markRead", firstAccount)] });
            }}
            data-testid="rule-add-action"
          >
            {t("comfort.rules.addAction")}
          </Button>
        </div>
      </fieldset>
      <Switch
        label={t("comfort.rules.stop.label")}
        description={t("comfort.rules.stop.description")}
        checked={input.stop}
        onChange={(stop) => {
          set({ stop });
        }}
      />
      <Switch
        label={t("comfort.rules.enabled")}
        checked={input.enabled}
        onChange={(enabled) => {
          set({ enabled });
        }}
      />
      {tried && problem && problem !== "name" && (
        <p role="alert" className="text-13 text-danger" data-testid="rule-problem">
          {t(`comfort.rules.problems.${problem}`)}
        </p>
      )}
      {error && <ErrorNotice context={t("comfort.rules.saveFailed")} error={error} />}
      <div className="flex gap-2">
        <Button type="submit" disabled={saving} data-testid="rule-save">
          {t("comfort.rules.save")}
        </Button>
        <Button variant="secondary" onClick={onCancel}>
          {t("comfort.rules.cancel")}
        </Button>
      </div>
    </form>
  );
}

function MoveTarget({
  action,
  accountOptions,
  onChange,
}: {
  action: Extract<RuleAction, { type: "move" }>;
  accountOptions: readonly { value: string; label: string }[];
  onChange: (action: Extract<RuleAction, { type: "move" }>) => void;
}) {
  const { t } = useTranslation();
  const mailboxes = useMailboxes(action.accountId || null);
  const folders = (mailboxes.data ?? []).filter((mailbox) => mailbox.selectable);
  const folderOptions = [
    { value: "", label: t("comfort.rules.chooseFolder") },
    ...folders.map((mailbox) => ({
      value: mailbox.path,
      label:
        mailbox.role === "custom"
          ? mailbox.path
          : t("comfort.rules.folderWithRole", { role: t(`roles.${mailbox.role}`), path: mailbox.path }),
    })),
  ];
  return (
    <>
      <Select<string>
        label={t("comfort.rules.accountLabel")}
        value={action.accountId}
        options={accountOptions}
        onChange={(accountId) => {
          onChange({ type: "move", accountId, path: "" });
        }}
      />
      <Select<string>
        label={t("comfort.rules.folderLabel")}
        value={action.path}
        options={folderOptions}
        onChange={(path) => {
          onChange({ ...action, path });
        }}
        data-testid="rule-move-folder"
      />
    </>
  );
}

