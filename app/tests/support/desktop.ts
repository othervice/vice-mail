import type { Autostart, BackgroundSettings, DesktopTexts, NotificationPause, PauseNotifications } from "../../src/services/ipc";

/** Stage 21 (decisions 0069, 0070): what the fake core answers for running in the background. */
export interface DesktopFakeOptions {
  background?: Partial<BackgroundSettings>;
  /** Default: an installed build, not registered. */
  autostart?: Partial<Autostart>;
  pause?: NotificationPause;
  /** `set_autostart` fails with this code (the system refused). */
  autostartFails?: "internal" | "invalid_argument";
}

export interface DesktopFakeState {
  readonly background: Required<BackgroundSettings>;
  readonly autostart: Autostart;
  readonly pause: NotificationPause;
  /** The texts the UI sent for the tray menu, last first. */
  readonly texts: DesktopTexts[];
  /** Answers to the close note: `false` into the background, `true` quit. */
  readonly closes: boolean[];
  readonly readyCalls: number;
}

const HOUR = 3_600_000;

export function createDesktopFake(
  options: DesktopFakeOptions,
  deps: { reject: (code: "internal" | "invalid_argument") => unknown },
) {
  let background: Required<BackgroundSettings> = { keepRunning: true, menuBarIcon: true, ...options.background };
  let autostart: Autostart = { enabled: false, available: true, ...options.autostart };
  let pause: NotificationPause = options.pause ?? { until: null, localTime: null };
  const texts: DesktopTexts[] = [];
  const closes: boolean[] = [];
  let readyCalls = 0;

  function paused(choice: PauseNotifications): NotificationPause {
    const now = Date.now();
    switch (choice) {
      case "resume":
        return { until: null, localTime: null };
      case "oneHour": {
        const end = new Date(now + HOUR);
        return { until: now + HOUR, localTime: { hour: end.getHours(), minute: end.getMinutes(), laterDay: false } };
      }
      case "untilTomorrow": {
        const end = new Date(now);
        end.setDate(end.getDate() + 1);
        end.setHours(8, 0, 0, 0);
        return { until: end.getTime(), localTime: { hour: 8, minute: 0, laterDay: true } };
      }
    }
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_background_settings":
        return { ...background };
      case "set_background_settings":
        background = { ...background, ...(args["settings"] as BackgroundSettings) };
        return { ...background };
      case "get_autostart":
        return { ...autostart };
      case "set_autostart":
        if (options.autostartFails) return deps.reject(options.autostartFails);
        if (!autostart.available) return deps.reject("invalid_argument");
        autostart = { ...autostart, enabled: args["enabled"] === true };
        return { ...autostart };
      case "get_notification_pause":
        return { ...pause };
      case "pause_notifications":
        pause = paused(args["choice"] as PauseNotifications);
        return { ...pause };
      case "set_desktop_texts":
        texts.unshift(args["texts"] as DesktopTexts);
        return null;
      case "main_window_ready":
        readyCalls += 1;
        return null;
      case "close_main_window":
        closes.push(args["quit"] === true);
        return null;
      default:
        return undefined;
    }
  }

  const state: DesktopFakeState = {
    get background() {
      return background;
    },
    get autostart() {
      return autostart;
    },
    get pause() {
      return pause;
    },
    texts,
    closes,
    get readyCalls() {
      return readyCalls;
    },
  };
  return { handle, state };
}
