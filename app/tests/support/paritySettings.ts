// Stage 16 part of the fake core for the settings (decision 0033): the notification sounds of the
// system the app runs on. Blocked senders are answered by ./parity.ts.
import type { NotificationSoundChoices } from "../../src/services/ipc";

/** What the core answers on macOS (decision 0033: `Default` and the alert sounds of /System/Library/Sounds). */
export const MACOS_SOUNDS: NotificationSoundChoices = {
  platform: "macos",
  sounds: ["Default", "Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink"],
};

export const WINDOWS_SOUNDS: NotificationSoundChoices = {
  platform: "windows",
  sounds: ["Default", "IM", "Mail", "Reminder", "SMS"],
};

export interface ParitySettingsFakeOptions {
  /** Default: the macOS list. */
  soundChoices?: NotificationSoundChoices;
}

export function createParitySettingsFake(options: ParitySettingsFakeOptions) {
  const choices = options.soundChoices ?? MACOS_SOUNDS;
  return {
    handle(cmd: string): unknown {
      switch (cmd) {
        case "get_notification_sound_choices":
          return { platform: choices.platform, sounds: [...choices.sounds] } satisfies NotificationSoundChoices;
        default:
          return undefined;
      }
    },
  };
}
