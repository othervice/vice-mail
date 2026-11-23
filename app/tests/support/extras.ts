// Stage 9 part B of the fake core: unsubscribing, calendar invitations, `.eml` export, printing,
// the default mail app and `mailto:` launch links, with the command names and argument shapes of
// the IPC contract (stage9-ipc-contract.md, decision 0020).
import type {
  CalendarEvent,
  CalendarInviteInfo,
  ComposeRequest,
  DefaultMailApp,
  DefaultMailAppOutcome,
  MessageExtras,
  UnsubscribeMethod,
  UnsubscribeOptions,
  UnsubscribeOutcome,
} from "../../src/services/ipc";

export interface ExtrasFakeOptions {
  /** `getMessageExtras` by message id; default: nothing to offer, body complete. */
  extras?: Record<number, Partial<MessageExtras>>;
  /** `unsubscribe` answers by `<messageId>/<method>`; default by method (see `unsubscribe`). */
  unsubscribeOutcomes?: Record<string, UnsubscribeOutcome>;
  /** `exportMessage`: the saved path, or null for a cancelled dialog. */
  exportPath?: string | null;
  defaultMailApp?: DefaultMailApp;
  /** What `setDefaultMailApp` did; `done` also makes the app the default. */
  setDefaultOutcome?: DefaultMailAppOutcome;
  /** `takeLaunchLinks` answers these once, then nothing. */
  launchLinks?: ComposeRequest[];
}

export interface ExtrasFakeState {
  /** Unsubscribe requests that reached the core, in order (a counting server stands behind it). */
  readonly unsubscribes: { messageId: number; method: UnsubscribeMethod }[];
  readonly openedInvites: { messageId: number; partId: number }[];
  readonly exports: number[];
  readonly prints: number;
  readonly defaultMailApp: DefaultMailApp;
}

export const NO_UNSUBSCRIBE: UnsubscribeOptions = { oneClickHost: null, mailto: null, web: null };

export function makeComposeRequest(overrides: Partial<ComposeRequest> = {}): ComposeRequest {
  return { to: [], cc: [], bcc: [], subject: null, body: null, inReplyTo: null, notices: [], ...overrides };
}

export function makeCalendarEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    uid: "event-1@example.org",
    summary: "Team meeting",
    location: null,
    description: null,
    organizer: null,
    attendees: [],
    attendeeCount: 0,
    start: null,
    end: null,
    recurrence: null,
    recurrenceId: null,
    status: null,
    sequence: 0,
    cancelled: false,
    ...overrides,
  };
}

export function makeInvite(events: CalendarEvent[], overrides: Partial<CalendarInviteInfo["invite"]> = {}, partId = 2): CalendarInviteInfo {
  return { partId, invite: { method: "REQUEST", events, problems: [], ...overrides } };
}

function extrasOf(messageId: number, given: Partial<MessageExtras> | undefined): MessageExtras {
  return {
    category: { messageId, decision: null, address: null, domain: null, list: null },
    unsubscribe: NO_UNSUBSCRIBE,
    invites: [],
    bodyComplete: true,
    ...given,
  };
}

export function createExtrasFake(options: ExtrasFakeOptions) {
  const extras = new Map(Object.entries(options.extras ?? {}).map(([id, value]) => [Number(id), value]));
  const unsubscribes: { messageId: number; method: UnsubscribeMethod }[] = [];
  const openedInvites: { messageId: number; partId: number }[] = [];
  const exports: number[] = [];
  let prints = 0;
  let defaultMailApp: DefaultMailApp = options.defaultMailApp ?? { isDefault: false, direct: true };
  let launchLinks = [...(options.launchLinks ?? [])];

  function unsubscribe(messageId: number, method: UnsubscribeMethod): UnsubscribeOutcome {
    unsubscribes.push({ messageId, method });
    const fixed = options.unsubscribeOutcomes?.[`${String(messageId)}/${method}`];
    if (fixed) return fixed;
    const offered = extras.get(messageId)?.unsubscribe ?? NO_UNSUBSCRIBE;
    switch (method) {
      case "oneClick":
        return { kind: "requested", status: 200 };
      case "mailto":
        return {
          kind: "compose",
          request: makeComposeRequest({ to: [{ name: null, email: offered.mailto ?? "leave@lists.example.org" }], subject: "unsubscribe" }),
        };
      case "web":
        return { kind: "opened" };
    }
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_message_extras": {
        const messageId = args["messageId"] as number;
        return extrasOf(messageId, extras.get(messageId));
      }
      case "unsubscribe":
        return unsubscribe(args["messageId"] as number, args["method"] as UnsubscribeMethod);
      case "open_invite":
        openedInvites.push({ messageId: args["messageId"] as number, partId: args["partId"] as number });
        return null;
      case "export_message":
        exports.push(args["messageId"] as number);
        return options.exportPath === undefined ? "/Users/ada/Documents/Hello.eml" : options.exportPath;
      case "print_window":
        prints += 1;
        return null;
      case "get_default_mail_app":
        return { ...defaultMailApp };
      case "set_default_mail_app": {
        const outcome = options.setDefaultOutcome ?? "done";
        if (outcome === "done") defaultMailApp = { ...defaultMailApp, isDefault: true };
        return outcome;
      }
      case "take_launch_links": {
        const links = launchLinks;
        launchLinks = [];
        return links;
      }
      default:
        return undefined;
    }
  }

  /** Replaces what `getMessageExtras` answers for a message (its body arrived); the test then emits. */
  function setExtras(messageId: number, value: Partial<MessageExtras>): void {
    extras.set(messageId, value);
  }

  const state: ExtrasFakeState = {
    unsubscribes,
    openedInvites,
    exports,
    get prints() {
      return prints;
    },
    get defaultMailApp() {
      return defaultMailApp;
    },
  };

  return { handle, setExtras, state };
}

export type ExtrasFake = ReturnType<typeof createExtrasFake>;
