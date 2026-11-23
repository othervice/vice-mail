import { Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { IconButton } from "../../components/IconButton";
import { SettingsSection } from "../../components/SettingsSection";
import { Switch } from "../../components/Switch";
import type { InboxSettings as Settings, SenderRule } from "../../services/ipc";
import { BlockedSenders } from "../blocked/BlockedSenders";
import { LocalRules } from "./LocalRules";
import { UnreadFirstSettings } from "../organize/UnreadFirstSettings";
import { useDeleteSenderRule, useInboxSettings, useSenderRules, useSetInboxSettings } from "./queries";

/** The core's defaults (everything on), for fields an older store left out. */
export const INBOX_DEFAULTS: Required<Settings> = { categories: true, bundleNotifications: true, bundleNewsletters: true };

function withDefaults(settings: Settings | undefined): Required<Settings> {
  return { ...INBOX_DEFAULTS, ...settings };
}

/**
 * Settings → Inbox: sorting into categories (off: purely by date), the bundles per category, and
 * the user's corrections per sender, list or domain ("always as …", "important").
 */
export function InboxSettings() {
  const { t } = useTranslation();
  const settings = useInboxSettings();
  const save = useSetInboxSettings();
  const current = withDefaults(settings.data);
  const update = (patch: Partial<Settings>) => {
    save.mutate({ ...current, ...patch });
  };
  return (
    <div className="flex flex-col gap-10">
      <SettingsSection title={t("settings.inbox.title")}>
        {settings.isError && (
          <ErrorNotice
            context={t("comfort.settings.loadFailed")}
            error={settings.error}
            onRetry={() => {
              void settings.refetch();
            }}
          />
        )}
        <Switch
          label={t("comfort.settings.categories.label")}
          description={t("comfort.settings.categories.description")}
          checked={current.categories}
          disabled={!settings.data}
          onChange={(categories) => {
            update({ categories });
          }}
          data-testid="inbox-categories"
        />
        <Switch
          label={t("comfort.settings.bundleNotifications.label")}
          description={t("comfort.settings.bundleNotifications.description")}
          checked={current.bundleNotifications}
          disabled={!settings.data || !current.categories}
          onChange={(bundleNotifications) => {
            update({ bundleNotifications });
          }}
          data-testid="inbox-bundle-notifications"
        />
        <Switch
          label={t("comfort.settings.bundleNewsletters.label")}
          description={t("comfort.settings.bundleNewsletters.description")}
          checked={current.bundleNewsletters}
          disabled={!settings.data || !current.categories}
          onChange={(bundleNewsletters) => {
            update({ bundleNewsletters });
          }}
          data-testid="inbox-bundle-newsletters"
        />
        {save.isError && <ErrorNotice context={t("comfort.settings.saveFailed")} error={save.error} />}
      </SettingsSection>
      {/* Stage 16: the views that show their unread mail first (device setting). */}
      <UnreadFirstSettings />
      <SenderRules />
      {/* Stage 16: blocked senders, with "Unblock". */}
      <BlockedSenders />
      <LocalRules />
    </div>
  );
}

function SenderRules() {
  const { t } = useTranslation();
  const rules = useSenderRules();
  const remove = useDeleteSenderRule();
  return (
    <SettingsSection title={t("comfort.senderRules.title")}>
      <p className="text-13 text-secondary">{t("comfort.senderRules.description")}</p>
      {rules.isError && (
        <ErrorNotice
          context={t("comfort.senderRules.loadFailed")}
          error={rules.error}
          onRetry={() => {
            void rules.refetch();
          }}
        />
      )}
      {rules.data?.length === 0 && (
        <p className="text-13 text-secondary" data-testid="sender-rules-empty">
          {t("comfort.senderRules.empty")}
        </p>
      )}
      {rules.data && rules.data.length > 0 && (
        <ul className="flex flex-col divide-y divide-border rounded-card border border-border" data-testid="sender-rules">
          {rules.data.map((rule) => (
            <SenderRuleRow
              key={rule.id}
              rule={rule}
              disabled={remove.isPending}
              onDelete={() => {
                remove.mutate(rule.id);
              }}
            />
          ))}
        </ul>
      )}
      {remove.isError && <ErrorNotice context={t("comfort.senderRules.deleteFailed")} error={remove.error} />}
    </SettingsSection>
  );
}

function SenderRuleRow({ rule, disabled, onDelete }: { rule: SenderRule; disabled: boolean; onDelete: () => void }) {
  const { t } = useTranslation();
  const what = [
    rule.category && t("comfort.senderRules.always", { category: t(`comfort.categories.${rule.category}`) }),
    rule.important && t("comfort.senderRules.important"),
  ]
    .filter(Boolean)
    .join(", ");
  return (
    <li className="flex items-center gap-3 px-4 py-2" data-testid="sender-rule">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-14 text-primary">{rule.key.value}</span>
        <span className="text-12 text-secondary">
          {t(`comfort.senderRules.kind.${rule.key.kind}`)} · {what}
        </span>
      </div>
      <IconButton
        icon={Trash2}
        label={t("comfort.senderRules.delete", { value: rule.key.value })}
        disabled={disabled}
        onClick={onDelete}
      />
    </li>
  );
}
