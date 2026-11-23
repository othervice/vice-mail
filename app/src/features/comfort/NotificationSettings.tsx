import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ChoiceGroup, type ChoiceOption } from "../../components/ChoiceGroup";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSection } from "../../components/SettingsSection";
import { Switch } from "../../components/Switch";
import type { BadgeMode, NotificationSettings as Settings, NotifyCategories, QuietHours } from "../../services/ipc";
import { useAccounts } from "../accounts/queries";
import { useNotificationSettings, useSetNotificationSettings } from "./queries";

/** The categories new mail can announce itself for, in the order of the settings. */
const NOTIFY_CATEGORIES = ["people", "priority", "notifications", "newsletters"] as const satisfies readonly (keyof NotifyCategories)[];

type Full = Required<Omit<Settings, "categories" | "quietHours">> & {
  categories: Required<NotifyCategories>;
  quietHours: QuietHours;
};

/** The core's defaults (decision 0020): people and priority, failed sends, snoozes, quiet 22–7 off. */
export const NOTIFICATION_DEFAULTS: Full = {
  enabled: true,
  newMail: true,
  categories: { people: true, priority: true, notifications: false, newsletters: false },
  failedSends: true,
  snoozes: true,
  quietHours: { enabled: false, startMinute: 22 * 60, endMinute: 7 * 60 },
  hideContent: false,
  mutedAccounts: [],
  badge: "inboxUnread",
};

export function withNotificationDefaults(settings: Settings | undefined): Full {
  return {
    ...NOTIFICATION_DEFAULTS,
    ...settings,
    categories: { ...NOTIFICATION_DEFAULTS.categories, ...settings?.categories },
    quietHours: settings?.quietHours ?? NOTIFICATION_DEFAULTS.quietHours,
    mutedAccounts: settings?.mutedAccounts ?? [],
  };
}

/** "22:00" for a minute of the day, and back; null for anything else. */
export function minuteToTime(minute: number): string {
  const clamped = ((Math.round(minute) % 1440) + 1440) % 1440;
  return `${String(Math.floor(clamped / 60)).padStart(2, "0")}:${String(clamped % 60).padStart(2, "0")}`;
}

export function timeToMinute(value: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 23 || minutes > 59) return null;
  return hours * 60 + minutes;
}

function TimeField({ label, minute, disabled, onChange, ...rest }: {
  label: string;
  minute: number;
  disabled: boolean;
  onChange: (minute: number) => void;
  "data-testid": string;
}) {
  const id = useId();
  return (
    <div className="flex items-center gap-2">
      <label htmlFor={id} className="text-13 text-secondary">
        {label}
      </label>
      <input
        id={id}
        type="time"
        value={minuteToTime(minute)}
        disabled={disabled}
        onChange={(event) => {
          const next = timeToMinute(event.target.value);
          if (next !== null) onChange(next);
        }}
        data-testid={rest["data-testid"]}
        className="rounded-control border border-border bg-surface px-2 py-1 text-14 text-primary disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-focus-ring"
      />
    </div>
  );
}

/**
 * Settings → Notifications: what announces itself (new mail by category, failed sends, returned
 * snoozes), quiet hours, hiding the content, accounts that stay silent, and the Dock or taskbar
 * counter. The core decides and shows the notifications; this only stores the choices.
 */
export function NotificationSettings() {
  const { t } = useTranslation();
  const settings = useNotificationSettings();
  const accounts = useAccounts();
  const save = useSetNotificationSettings();
  const current = withNotificationDefaults(settings.data);
  const ready = Boolean(settings.data);
  const off = !ready || !current.enabled;
  const update = (patch: Partial<Full>) => {
    save.mutate({ ...current, ...patch });
  };
  const category = (key: keyof NotifyCategories, value: boolean) => {
    update({ categories: { ...current.categories, [key]: value } });
  };
  const badgeOptions: ChoiceOption<BadgeMode>[] = [
    { value: "off", label: t("comfort.notifications.badge.off") },
    { value: "inboxUnread", label: t("comfort.notifications.badge.inboxUnread") },
    { value: "peopleUnread", label: t("comfort.notifications.badge.peopleUnread") },
  ];

  return (
    <SettingsSection title={t("settings.notifications.title")}>
      {settings.isError && (
        <ErrorNotice
          context={t("comfort.notifications.loadFailed")}
          error={settings.error}
          onRetry={() => {
            void settings.refetch();
          }}
        />
      )}
      <Switch
        label={t("comfort.notifications.enabled.label")}
        description={t("comfort.notifications.enabled.description")}
        checked={current.enabled}
        disabled={!ready}
        onChange={(enabled) => {
          update({ enabled });
        }}
        data-testid="notify-enabled"
      />
      <fieldset className="flex flex-col gap-4" disabled={off}>
        <legend className="pb-2 text-14 font-semibold text-primary">{t("comfort.notifications.newMail.legend")}</legend>
        <Switch
          label={t("comfort.notifications.newMail.label")}
          checked={current.newMail}
          disabled={off}
          onChange={(newMail) => {
            update({ newMail });
          }}
          data-testid="notify-new-mail"
        />
        {NOTIFY_CATEGORIES.map((key) => (
          <Switch
            key={key}
            label={t(`comfort.notifications.categories.${key}`)}
            checked={current.categories[key]}
            disabled={off || !current.newMail}
            onChange={(value) => {
              category(key, value);
            }}
            data-testid={`notify-category-${key}`}
          />
        ))}
        <Switch
          label={t("comfort.notifications.failedSends")}
          checked={current.failedSends}
          disabled={off}
          onChange={(failedSends) => {
            update({ failedSends });
          }}
          data-testid="notify-failed-sends"
        />
        <Switch
          label={t("comfort.notifications.snoozes")}
          checked={current.snoozes}
          disabled={off}
          onChange={(snoozes) => {
            update({ snoozes });
          }}
          data-testid="notify-snoozes"
        />
      </fieldset>
      <div className="flex flex-col gap-3">
        <Switch
          label={t("comfort.notifications.quiet.label")}
          description={t("comfort.notifications.quiet.description")}
          checked={current.quietHours.enabled}
          disabled={off}
          onChange={(enabled) => {
            update({ quietHours: { ...current.quietHours, enabled } });
          }}
          data-testid="notify-quiet"
        />
        <div className="flex flex-wrap gap-4">
          <TimeField
            label={t("comfort.notifications.quiet.from")}
            minute={current.quietHours.startMinute}
            disabled={off || !current.quietHours.enabled}
            onChange={(startMinute) => {
              update({ quietHours: { ...current.quietHours, startMinute } });
            }}
            data-testid="notify-quiet-start"
          />
          <TimeField
            label={t("comfort.notifications.quiet.to")}
            minute={current.quietHours.endMinute}
            disabled={off || !current.quietHours.enabled}
            onChange={(endMinute) => {
              update({ quietHours: { ...current.quietHours, endMinute } });
            }}
            data-testid="notify-quiet-end"
          />
        </div>
      </div>
      <Switch
        label={t("comfort.notifications.hideContent.label")}
        description={t("comfort.notifications.hideContent.description")}
        checked={current.hideContent}
        disabled={off}
        onChange={(hideContent) => {
          update({ hideContent });
        }}
        data-testid="notify-hide-content"
      />
      {accounts.data && accounts.data.length > 0 && (
        <fieldset className="flex flex-col gap-4" disabled={off}>
          <legend className="pb-2 text-14 font-semibold text-primary">{t("comfort.notifications.accounts")}</legend>
          {accounts.data.map((account) => (
            <Switch
              key={account.id}
              label={account.displayName ? t("nav.account", { name: account.displayName, email: account.email }) : account.email}
              checked={!current.mutedAccounts.includes(account.id)}
              disabled={off}
              onChange={(on) => {
                const others = current.mutedAccounts.filter((id) => id !== account.id);
                update({ mutedAccounts: on ? others : [...others, account.id] });
              }}
              data-testid={`notify-account-${account.id}`}
            />
          ))}
        </fieldset>
      )}
      <ChoiceGroup
        label={t("comfort.notifications.badge.label")}
        description={t("comfort.notifications.badge.description")}
        value={current.badge}
        options={badgeOptions}
        onChange={(badge) => {
          update({ badge });
        }}
      />
      {save.isError && <ErrorNotice context={t("comfort.notifications.saveFailed")} error={save.error} />}
    </SettingsSection>
  );
}
