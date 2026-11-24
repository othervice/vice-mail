// Stage 9 part of the fake core: the sorted inbox, snooze, pins, tasks, sender rules and the
// notification settings, with the command names and argument shapes of the IPC contract.
import type {
  DateSection,
  InboxPage,
  InboxRow,
  InboxScope,
  InboxSettings,
  MailRule,
  MessageCategoryInfo,
  MessageListItem,
  MessagePage,
  NotificationSettings,
  NotificationTexts,
  RuleInput,
  SenderKey,
  SenderRule,
  SnoozeChoice,
  SnoozeTimes,
  TaskMark,
} from "../../src/services/ipc";

export interface ComfortFakeOptions {
  inboxSettings?: InboxSettings;
  /**
   * `listInbox` answers per mailbox id, one per page (the cursor's `before` is the page index).
   * Without them the inbox is the folder's messages by date, without waiting snoozes, with the
   * pinned and returned ones on top, as the core does with categories off.
   */
  inboxPages?: Record<number, InboxPage[]>;
  /** `listInboxFiltered` with `priority`, per mailbox id. */
  priority?: Record<number, MessageListItem[]>;
  /** `getMessageCategory` by message id; default: not classified yet, no keys. */
  categories?: Record<number, MessageCategoryInfo>;
  senderRules?: SenderRule[];
  snoozeTimes?: SnoozeTimes;
  notificationSettings?: NotificationSettings;
  rules?: MailRule[];
}

export interface ComfortFake {
  handle: (cmd: string, args: Record<string, unknown>) => unknown;
  readonly pinned: ReadonlySet<number>;
  readonly snoozed: ReadonlyMap<number, { until: number; returned: boolean }>;
  readonly tasks: ReadonlyMap<number, TaskMark>;
  readonly notificationTexts: NotificationTexts | null;
  readonly inboxSettings: InboxSettings;
  readonly notificationSettings: NotificationSettings;
  readonly senderRules: SenderRule[];
  readonly rules: MailRule[];
}

/** Answers of `getSnoozeTimes` in tests: fixed, so the menu's times are known. */
export const SNOOZE_TIMES: SnoozeTimes = {
  laterToday: Date.UTC(2026, 10, 23, 18, 0),
  tomorrow: Date.UTC(2026, 10, 24, 7, 0),
  weekend: Date.UTC(2026, 10, 28, 8, 0),
  nextWeek: Date.UTC(2026, 10, 30, 7, 0),
};

/** The core's date sections: today (and the future), yesterday, this week from Monday, months. */
export function inboxSection(date: number, now = new Date()): DateSection {
  const day = new Date(date);
  day.setHours(0, 0, 0, 0);
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const monday = new Date(today);
  monday.setDate(today.getDate() - ((today.getDay() + 6) % 7));
  if (day.getTime() >= today.getTime()) return { kind: "today", start: today.getTime() };
  if (day.getTime() === yesterday.getTime()) return { kind: "yesterday", start: yesterday.getTime() };
  if (day.getTime() >= monday.getTime()) return { kind: "thisWeek", start: monday.getTime() };
  return { kind: "month", start: new Date(day.getFullYear(), day.getMonth(), 1).getTime() };
}

/** A message row of `listInbox`. */
export function messageRow(item: MessageListItem, section: DateSection = inboxSection(item.date)): InboxRow {
  return { kind: "message", item, section };
}

const NOTIFICATION_DEFAULTS: NotificationSettings = {
  enabled: true,
  newMail: true,
  categories: { people: true, priority: true, notifications: false, newsletters: false },
  failedSends: true,
  snoozes: true,
  quietHours: { enabled: false, startMinute: 1320, endMinute: 420 },
  hideContent: false,
  mutedAccounts: [],
  badge: "inboxUnread",
};

export function createComfortFake(
  options: ComfortFakeOptions,
  core: {
    messagesIn: (mailboxId: number) => MessageListItem[];
    listMessages: (mailboxId: number, cursor: number | null, limit: number) => MessagePage;
    reject: (code: "invalid_argument" | "not_found") => Promise<never>;
    /** The role of a mailbox, for the Flagged view (it leaves Trash and Junk out). */
    roleOf?: (mailboxId: number) => string | undefined;
  },
): ComfortFake {
  let inboxSettings: InboxSettings = options.inboxSettings ?? {
    categories: true,
    bundleNotifications: true,
    bundleNewsletters: true,
  };
  let notificationSettings: NotificationSettings = options.notificationSettings ?? NOTIFICATION_DEFAULTS;
  let notificationTexts: NotificationTexts | null = null;
  let senderRules = [...(options.senderRules ?? [])];
  const pinned = new Set<number>();
  const snoozed = new Map<number, { until: number; returned: boolean }>();
  const tasks = new Map<number, TaskMark>();
  let ruleId = 1;
  let rules: MailRule[] = [...(options.rules ?? [])];

  const marked = (item: MessageListItem): MessageListItem => {
    const snooze = snoozed.get(item.id);
    const task = tasks.get(item.id);
    return {
      ...item,
      pinned: item.pinned || pinned.has(item.id),
      ...(snooze && { snooze }),
      ...(task && { task }),
    };
  };

  const scopeMailbox = (scope: InboxScope) => (scope.kind === "mailbox" ? scope.mailboxId : 1);

  const all = () => [1, 2, 3, 4, 5, 6, 7, 8, 9].flatMap((id) => core.messagesIn(id)).map(marked);

  function listInbox(scope: InboxScope, cursor: { before: number; bundles: string | null } | null, limit: number): InboxPage {
    const mailboxId = scopeMailbox(scope);
    const pages = options.inboxPages?.[mailboxId];
    if (pages) {
      const index = cursor?.before ?? 0;
      const page = pages[index];
      if (!page) return { rows: [], nextCursor: null, top: null, categories: inboxSettings.categories ?? true, conversations: [] };
      return { ...page, nextCursor: index + 1 < pages.length ? { before: index + 1, bundles: null } : null };
    }
    const folder = core.listMessages(mailboxId, cursor?.before ?? null, limit);
    const items = folder.items.map(marked);
    const shown = items.filter((item) => !item.pinned && !(item.snooze && !item.snooze.returned) && !item.snooze?.returned);
    const top =
      cursor === null
        ? {
            pinned: core.messagesIn(mailboxId).map(marked).filter((item) => item.pinned),
            returned: core.messagesIn(mailboxId).map(marked).filter((item) => item.snooze?.returned),
            priority: [],
            priorityTotal: 0,
            priorityUnread: 0,
          }
        : null;
    return {
      rows: shown.map((item) => messageRow(item)),
      nextCursor: folder.nextCursor === null ? null : { before: folder.nextCursor, bundles: null },
      top,
      categories: false,
      conversations: [],
    };
  }

  const ids = (args: Record<string, unknown>) => args["messageIds"] as number[];

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "list_inbox":
        return listInbox(
          args["scope"] as InboxScope,
          args["cursor"] as { before: number; bundles: string | null } | null,
          args["limit"] as number,
        );
      case "list_inbox_filtered": {
        const items = options.priority?.[scopeMailbox(args["scope"] as InboxScope)] ?? [];
        return { items, nextCursor: null } satisfies MessagePage;
      }
      case "get_inbox_settings":
        return inboxSettings;
      case "set_inbox_settings":
        inboxSettings = args["settings"] as InboxSettings;
        return inboxSettings;
      case "get_message_category": {
        const messageId = args["messageId"] as number;
        return (
          options.categories?.[messageId] ??
          ({ messageId, decision: null, address: null, domain: null, list: null } satisfies MessageCategoryInfo)
        );
      }
      case "set_sender_category":
      case "set_sender_important": {
        const key = args["key"] as SenderKey;
        const existing = senderRules.find((rule) => rule.key.kind === key.kind && rule.key.value === key.value);
        const base: SenderRule = existing ?? {
          id: `00000000-0000-4000-8000-${String(ruleId++).padStart(12, "0")}`,
          key,
          category: null,
          important: false,
          updatedAt: Date.now(),
        };
        const next: SenderRule =
          cmd === "set_sender_category"
            ? { ...base, category: args["category"] as SenderRule["category"] }
            : { ...base, important: args["important"] as boolean };
        senderRules = senderRules.filter((rule) => rule.id !== next.id);
        if (next.category === null && !next.important) return null;
        senderRules.push(next);
        return next;
      }
      case "list_sender_rules":
        return senderRules;
      case "delete_sender_rule": {
        const before = senderRules.length;
        senderRules = senderRules.filter((rule) => rule.id !== args["id"]);
        return senderRules.length === before ? core.reject("not_found") : null;
      }
      case "get_snooze_times":
        return options.snoozeTimes ?? SNOOZE_TIMES;
      case "snooze_messages": {
        const choice = args["choice"] as SnoozeChoice;
        const times = options.snoozeTimes ?? SNOOZE_TIMES;
        const until =
          choice.kind === "at"
            ? choice.at
            : choice.kind === "laterToday"
              ? (times.laterToday ?? times.tomorrow)
              : times[choice.kind];
        for (const id of ids(args)) snoozed.set(id, { until, returned: false });
        return { until };
      }
      case "unsnooze_messages":
        for (const id of ids(args)) snoozed.delete(id);
        return null;
      case "set_pinned":
        for (const id of ids(args)) {
          if (args["pinned"]) pinned.add(id);
          else pinned.delete(id);
        }
        return null;
      case "set_task": {
        const mark = args["mark"] as TaskMark | null;
        for (const id of ids(args)) {
          if (mark) tasks.set(id, mark);
          else tasks.delete(id);
        }
        return null;
      }
      case "list_snoozed":
        return all()
          .filter((item) => item.snooze && !item.snooze.returned)
          .sort((a, b) => (a.snooze?.until ?? 0) - (b.snooze?.until ?? 0));
      case "list_pinned":
        return all().filter((item) => item.pinned);
      case "list_flagged": {
        const left = new Set(["trash", "junk"]);
        return all()
          .filter((item) => item.flagged && !left.has(core.roleOf?.(item.mailboxId) ?? ""))
          .sort((a, b) => b.date - a.date);
      }
      case "list_tasks": {
        const filter = args["filter"] as "open" | "done" | "all";
        return all().filter((item) => item.task && (filter === "all" || (filter === "done") === item.task.done));
      }
      case "list_rules":
        return rules;
      case "create_rule": {
        const input = args["input"] as RuleInput;
        if (rules.length >= 200) return core.reject("invalid_argument");
        const rule: MailRule = { ...input, id: `rule-${String(ruleId++)}`, position: rules.length };
        rules = [...rules, rule];
        return rule;
      }
      case "update_rule": {
        const existing = rules.find((rule) => rule.id === args["id"]);
        if (!existing) return core.reject("not_found");
        const rule: MailRule = { ...existing, ...(args["input"] as RuleInput) };
        rules = rules.map((r) => (r.id === rule.id ? rule : r));
        return rule;
      }
      case "delete_rule": {
        const before = rules.length;
        rules = rules.filter((rule) => rule.id !== args["id"]);
        return rules.length === before ? core.reject("not_found") : null;
      }
      case "reorder_rules": {
        const order = args["ids"] as string[];
        rules = order.flatMap((id, position) => {
          const rule = rules.find((r) => r.id === id);
          return rule ? [{ ...rule, position }] : [];
        });
        return rules;
      }
      case "get_notification_settings":
        return notificationSettings;
      case "set_notification_settings":
        notificationSettings = args["settings"] as NotificationSettings;
        return notificationSettings;
      case "set_notification_texts":
        notificationTexts = args["texts"] as NotificationTexts;
        return null;
      default:
        return undefined;
    }
  }

  return {
    handle,
    pinned,
    snoozed,
    tasks,
    get notificationTexts() {
      return notificationTexts;
    },
    get inboxSettings() {
      return inboxSettings;
    },
    get notificationSettings() {
      return notificationSettings;
    },
    get senderRules() {
      return senderRules;
    },
    get rules() {
      return rules;
    },
  };
}
