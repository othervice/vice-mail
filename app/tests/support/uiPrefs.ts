// Stage 8 (decision 0025): the fake core's UI preferences, window layouts and the commands that
// open a message or a draft in a window of its own, with the contract's rules.
import type { CoreWindowLayout, IpcError, ShortcutOverride, UiPreferences } from "../../src/services/ipc";

/**
 * The fake core's stored preferences. The real core's default for `conversations` is on
 * (decision 0039); the list tests of earlier stages are about single messages, so the fake core
 * starts with the conversation view off and the conversation tests turn it on
 * (`CONVERSATION_PREFERENCES`).
 */
export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  shortcuts: { macos: [], windows: [] },
  textSize: 100,
  recentCommands: [],
  aiPanel: "open",
  accentTheme: "orange",
  conversations: false,
};

/** Decision 0039: the core's own default, the conversation view on. */
export const CONVERSATION_PREFERENCES: UiPreferences = { ...DEFAULT_UI_PREFERENCES, conversations: true };

/**
 * The fake core's window layout. The real core shows the accounts-and-folders column by default;
 * the earlier tests are written for the icon rail, so the fake starts with it folded.
 */
export const DEFAULT_WINDOW_LAYOUT: CoreWindowLayout = {
  readingPane: "right",
  density: "comfortable",
  sidebarExpanded: false,
  sidebarWidth: 240,
};

/** What Tauri answers a window command the capability does not allow (a plain string). */
export const CLOSE_DENIED = "window.close not allowed. Permissions associated with this command: core:window:allow-close";

export interface UiFakeOptions {
  preferences?: UiPreferences;
  /** The layout of the window the test plays (the shell decides by the calling window). */
  layout?: CoreWindowLayout;
  /** The shell allows the window to close itself (the capability lacks it today). */
  canCloseWindow?: boolean;
}

export interface UiFakeState {
  readonly preferences: UiPreferences;
  readonly layout: CoreWindowLayout;
  /** `openMessageWindow` calls that opened (or focused) a window, as `<accountId>/<messageId>`. */
  readonly messageWindows: readonly string[];
  /** `openComposerWindow` calls that opened (or focused) a window, by draft id. */
  readonly composerWindows: readonly string[];
  /** How often this window asked to close. */
  readonly closeRequests: number;
}

interface Deps {
  /** The account of a stored message, if the fake core has it. */
  accountOfMessage: (messageId: number) => string | null;
  hasDraft: (draftId: string) => boolean;
  reject: (code: IpcError["code"], detail?: string | null) => Promise<never>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const COMMAND = /^[A-Za-z0-9._:-]{1,64}$/;

/** The core's `UiPreferences::validate`, as far as the tests need it; null when valid. */
function problemOf(preferences: UiPreferences): string | null {
  if (!Number.isInteger(preferences.textSize) || preferences.textSize < 80 || preferences.textSize > 150) {
    return `text size ${String(preferences.textSize)} is outside 80..=150`;
  }
  // Decision 0028: serde refuses anything else (a missing field too).
  if ((preferences.aiPanel as unknown) !== "open" && (preferences.aiPanel as unknown) !== "folded") {
    return `aiPanel ${String(preferences.aiPanel)} is neither open nor folded`;
  }
  // Stage 16 (decision 0036): only the six accent themes.
  if (!["teal", "blue", "violet", "rose", "orange", "green"].includes(preferences.accentTheme)) {
    return `accentTheme ${String(preferences.accentTheme)} is not a theme`;
  }
  // Decision 0039: serde refuses anything but a boolean (a missing field too).
  if (typeof (preferences.conversations as unknown) !== "boolean") return "conversations is not a boolean";
  const recent = preferences.recentCommands;
  if (recent.length > 5) return "at most 5 recent commands";
  if (new Set(recent).size !== recent.length) return "a recent command appears twice";
  if (recent.some((entry) => entry.length === 0 || entry.length > 200)) return "recent command length";
  for (const platform of ["macos", "windows"] as const) {
    const list = preferences.shortcuts[platform];
    if (list.length > 100) return `${platform}: at most 100 commands`;
    const seen = new Set<string>();
    for (const entry of list) {
      if (!COMMAND.test(entry.command)) return `${platform}: command "${entry.command}" is not valid`;
      if (seen.has(entry.command)) return `${platform}: command "${entry.command}" appears twice`;
      seen.add(entry.command);
      if (entry.shortcuts.length > 4) return `${platform}: "${entry.command}" has more than 4 shortcuts`;
      const keys = entry.shortcuts.map((s) => `${String(s.mod)}${String(s.shift)}${s.key}`);
      if (new Set(keys).size !== keys.length) return `${platform}: "${entry.command}" has the same shortcut twice`;
      if (entry.shortcuts.some((s) => s.key.length === 0 || s.key.length > 20)) return `${platform}: key length`;
      if (entry.shortcuts.some((s) => typeof s.shift !== "boolean" || typeof s.mod !== "boolean")) return `${platform}: shift/mod missing`;
    }
  }
  return null;
}

const sorted = (list: readonly ShortcutOverride[]) => [...list].sort((a, b) => a.command.localeCompare(b.command));

export function createUiFake(options: UiFakeOptions, deps: Deps) {
  let preferences: UiPreferences = options.preferences ?? DEFAULT_UI_PREFERENCES;
  let layout: CoreWindowLayout = options.layout ?? DEFAULT_WINDOW_LAYOUT;
  const messageWindows: string[] = [];
  const composerWindows: string[] = [];
  let closeRequests = 0;

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_ui_preferences":
        return preferences;
      case "set_ui_preferences": {
        const next = args["preferences"] as UiPreferences;
        const problem = problemOf(next);
        if (problem) return deps.reject("invalid_argument", problem);
        preferences = {
          ...next,
          shortcuts: { macos: sorted(next.shortcuts.macos), windows: sorted(next.shortcuts.windows) },
        };
        return preferences;
      }
      case "get_window_layout":
        return layout;
      case "set_window_layout": {
        const next = args["layout"] as CoreWindowLayout;
        // The core's `WindowLayout::validate`: the column is 200 to 320 px wide.
        if (!Number.isInteger(next.sidebarWidth) || next.sidebarWidth < 200 || next.sidebarWidth > 320) {
          return deps.reject("invalid_argument", `sidebar width ${String(next.sidebarWidth)} is outside 200 to 320 pixels`);
        }
        layout = next;
        return layout;
      }
      case "open_message_window": {
        const accountId = args["accountId"] as string;
        const messageId = args["messageId"] as number;
        if (deps.accountOfMessage(messageId) !== accountId) return deps.reject("not_found");
        messageWindows.push(`${accountId}/${String(messageId)}`);
        return null;
      }
      case "open_composer_window": {
        const draftId = args["draftId"] as string;
        if (!UUID.test(draftId)) return deps.reject("invalid_argument", "draft id");
        if (!deps.hasDraft(draftId)) return deps.reject("not_found");
        composerWindows.push(draftId);
        return null;
      }
      case "plugin:window|close":
        closeRequests += 1;
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return options.canCloseWindow ? null : Promise.reject(CLOSE_DENIED);
      default:
        return undefined;
    }
  }

  const state: UiFakeState = {
    get preferences() {
      return preferences;
    },
    get layout() {
      return layout;
    },
    messageWindows,
    composerWindows,
    get closeRequests() {
      return closeRequests;
    },
  };

  return { handle, state };
}
