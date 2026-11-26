import { useQuery } from "@tanstack/react-query";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Switch } from "../../components/Switch";
import { parityIpc, type Account, type NotificationSound as Sound, type SoundPlatform } from "../../services/ipc";
import { cx } from "../../components/cx";
import { SETTING_ANCHOR_CLASS, SETTING_BLOCK, SETTING_LEGEND_CLASS, settingAnchor } from "../../components/settingAnchor";

export const soundChoicesQueryKey = ["comfort", "soundChoices"] as const;

/** The sounds of the system the app runs on (fixed per platform, decision 0033). */
export function useNotificationSoundChoices() {
  return useQuery({
    queryKey: soundChoicesQueryKey,
    queryFn: () => parityIpc.getNotificationSoundChoices(),
    staleTime: Infinity,
  });
}

/** The core's name of the system's default sound (decision 0033); a name, not a UI text. */
const DEFAULT_SOUND = "Default";

/** The system's sound names that get a translated label; the rest are shown as the system names them. */
const LABELLED_SOUNDS = { Default: "default", IM: "im", Mail: "mail", Reminder: "reminder", SMS: "sms" } as const;

type PlatformField = keyof Pick<Required<Sound>, "macos" | "windows">;

function platformField(platform: SoundPlatform): PlatformField | null {
  return platform === "macos" ? "macos" : platform === "windows" ? "windows" : null;
}

interface NotificationSoundProps {
  sound: Required<Sound>;
  /** Notifications as a whole are off or not loaded yet. */
  off: boolean;
  accounts: readonly Account[];
  mutedAccounts: readonly string[];
  onChange: (sound: Required<Sound>) => void;
}

/**
 * Stage 16, Settings → Notifications: sound on or off, which system sound (only this system's
 * field changes; the other platform's choice goes back as it came, both are synced), and
 * accounts whose notifications show without a sound. No preview: the system plays a sound only
 * with a notification (decision 0033).
 */
export function NotificationSoundSettings({ sound, off, accounts, mutedAccounts, onChange }: NotificationSoundProps) {
  const { t } = useTranslation();
  const selectId = useId();
  const choices = useNotificationSoundChoices();
  const field = choices.data ? platformField(choices.data.platform) : null;
  const sounds = choices.data?.sounds ?? [];
  const silentOff = off || !sound.enabled;
  const label = (name: string) =>
    name in LABELLED_SOUNDS ? t(`sounds.names.${LABELLED_SOUNDS[name as keyof typeof LABELLED_SOUNDS]}`) : name;
  const current = field ? sound[field] : null;

  return (
    // Stage 22 (decision 0073): found by the settings search by its legend and its rows.
    <fieldset className="flex flex-col gap-4" disabled={off} data-testid="notify-sound" {...SETTING_BLOCK}>
      <legend {...settingAnchor(t("sounds.legend"))} data-setting-key="sounds.legend" className={cx("pb-2 text-14 font-semibold text-primary", SETTING_LEGEND_CLASS)}>
        {t("sounds.legend")}
      </legend>
      <Switch
        label={t("sounds.enabled.label")}
        description={t("sounds.enabled.description")}
        checked={sound.enabled}
        disabled={off}
        onChange={(enabled) => {
          onChange({ ...sound, enabled });
        }}
        data-testid="notify-sound-enabled"
      />
      {field && current !== null && sounds.length > 0 && (
        <div {...settingAnchor(t("sounds.choice"))} data-setting-key="sounds.choice" className={cx("flex flex-col gap-1", SETTING_ANCHOR_CLASS)}>
          <label htmlFor={selectId} className="text-14 font-medium text-primary">
            {t("sounds.choice")}
          </label>
          <select
            id={selectId}
            value={sounds.includes(current) ? current : DEFAULT_SOUND}
            disabled={silentOff}
            onChange={(event) => {
              onChange({ ...sound, [field]: event.target.value });
            }}
            className="h-8 w-fit min-w-(--ui-list-sender-width) rounded-control border border-border-strong bg-surface-raised px-2 text-13 text-primary disabled:opacity-50"
            data-testid="notify-sound-choice"
          >
            {sounds.map((name) => (
              <option key={name} value={name}>
                {label(name)}
              </option>
            ))}
          </select>
          <span className="text-13 text-secondary">{t("sounds.noPreview")}</span>
        </div>
      )}
      {accounts.length > 0 && (
        <fieldset className="flex flex-col gap-4" disabled={silentOff}>
          <legend {...settingAnchor(t("sounds.accounts"))} className={cx("pb-2 text-13 font-semibold text-secondary", SETTING_LEGEND_CLASS)}>
            {t("sounds.accounts")}
          </legend>
          {accounts.map((account) => {
            const muted = mutedAccounts.includes(account.id);
            return (
              <Switch
                key={account.id}
                findable={false}
                label={t("sounds.accountLabel", {
                  account: account.displayName
                    ? t("nav.account", { name: account.displayName, email: account.email })
                    : account.email,
                })}
                {...(muted && { description: t("sounds.mutedAccount") })}
                checked={!sound.silentAccounts.includes(account.id)}
                disabled={silentOff || muted}
                onChange={(on) => {
                  const others = sound.silentAccounts.filter((id) => id !== account.id);
                  onChange({ ...sound, silentAccounts: on ? others : [...others, account.id] });
                }}
                data-testid={`notify-sound-account-${account.id}`}
              />
            );
          })}
        </fieldset>
      )}
    </fieldset>
  );
}
