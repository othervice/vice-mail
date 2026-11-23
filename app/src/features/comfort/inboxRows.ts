import type {
  BundleSummary,
  DateSection,
  InboxPage,
  InboxTop,
  MessageCategory,
  MessageId,
  MessageListItem,
} from "../../services/ipc";
import type { Section } from "../messages/rows";

/** The blocks above the dated rows (decision 0020, "Oben"): pinned, returned snoozes, priority. */
export type InboxBlock = "pinned" | "returned" | "priority";

/** One bundle as the list shows it: the core's summary and every member loaded so far. */
export interface InboxBundle {
  /** `<category>-<section start>`, stable across pages and reloads. */
  key: string;
  category: MessageCategory;
  summary: BundleSummary;
  members: MessageListItem[];
}

/** Rows of the sorted inbox, before they become list rows (pure, tested without React). */
export type InboxListRow =
  | { kind: "block"; key: string; block: InboxBlock }
  | { kind: "section"; key: string; section: Section }
  | { kind: "message"; key: string; item: MessageListItem; block: InboxBlock | null; bundle: string | null }
  | { kind: "bundle"; key: string; bundle: InboxBundle; expanded: boolean }
  /** "Show all (n)" below the priority block, when there are more than it shows. */
  | { kind: "showAll"; key: string; total: number; unread: number };

/** A page of the inbox as the list reads it: every message of the page in list order, plus the rows. */
export interface InboxViewPage {
  items: MessageListItem[];
  page: InboxPage;
}

export function bundleKeyOf(summary: BundleSummary): string {
  return `${summary.key.category}-${String(summary.key.section)}`;
}

/** The core's date section as the list's section (months from the section's local start). */
export function sectionOfDate(section: DateSection): Section {
  if (section.kind !== "month") return { kind: section.kind };
  const start = new Date(section.start);
  return { kind: "month", year: start.getFullYear(), month: start.getMonth() };
}

function topItems(top: InboxTop | null): MessageListItem[] {
  return top ? [...top.pinned, ...top.returned, ...top.priority] : [];
}

/** Every message of one page in list order (blocks first, bundle members after their bundle). */
export function inboxPageItems(page: InboxPage): MessageListItem[] {
  const rows = page.rows.flatMap((row) => (row.kind === "message" ? [row.item] : row.members));
  // Priority messages come in the block and in the rows: count them once.
  const seen = new Set<MessageId>();
  return [...topItems(page.top), ...rows].filter((item) => {
    if (seen.has(item.id)) return false;
    seen.add(item.id);
    return true;
  });
}

type Dated =
  | { kind: "message"; item: MessageListItem; section: DateSection }
  | { kind: "bundle"; bundle: InboxBundle; section: DateSection };

/**
 * Builds the rows of the sorted inbox from its pages:
 * - the blocks of the first page on top (pinned, returned, priority with "Show all (n)");
 * - then the dated rows with a heading per date section;
 * - a bundle (`more` rows of later pages are appended to it and replace its summary) as one row,
 *   its members below it while it is expanded; a bundle with a single message is a plain row;
 * - each message once: one shown in a block is not repeated below (priority messages also come
 *   in the rows), and messages not in `visible` (an action is moving them away) are left out;
 * - decision 0039: `keyOf` names what is shown once, by default the message; conversation rows
 *   pass their thread, so a conversation of the priority block is not repeated below either.
 */
export function buildInboxRows(
  pages: readonly InboxPage[],
  expanded: ReadonlySet<string>,
  visible: ReadonlySet<MessageId>,
  keyOf: (item: MessageListItem) => string | number = (item) => item.id,
): InboxListRow[] {
  const rows: InboxListRow[] = [];
  const seen = new Set<string | number>();
  const taken = (item: MessageListItem) => seen.has(keyOf(item));
  const take = (item: MessageListItem) => {
    if (taken(item) || !visible.has(item.id)) return false;
    seen.add(keyOf(item));
    return true;
  };

  const top = pages[0]?.top ?? null;
  if (top) {
    const blocks: [InboxBlock, MessageListItem[]][] = [
      ["pinned", top.pinned],
      ["returned", top.returned],
      ["priority", top.priority],
    ];
    for (const [block, items] of blocks) {
      const shown = items.filter(take);
      if (shown.length === 0) continue;
      rows.push({ kind: "block", key: `block-${block}`, block });
      for (const item of shown) rows.push({ kind: "message", key: `message-${String(item.id)}`, item, block, bundle: null });
      if (block === "priority" && top.priorityTotal > top.priority.length) {
        rows.push({ kind: "showAll", key: "priority-all", total: top.priorityTotal, unread: top.priorityUnread });
      }
    }
  }

  // Bundles by key, so `more` rows of later pages extend the bundle where it already stands.
  const bundles = new Map<string, InboxBundle>();
  const dated: Dated[] = [];
  let lastSection: DateSection | null = null;
  for (const page of pages) {
    for (const row of page.rows) {
      if (row.kind === "message") {
        dated.push({ kind: "message", item: row.item, section: row.section });
        lastSection = row.section;
        continue;
      }
      const key = bundleKeyOf(row.summary);
      const existing = bundles.get(key);
      if (existing) {
        existing.members.push(...row.members);
        existing.summary = row.summary;
        continue;
      }
      // A `more` without its bundle (never expected) starts the bundle where it is.
      const section: DateSection | null = row.kind === "bundle" ? row.section : lastSection;
      if (!section) continue;
      const bundle: InboxBundle = { key, category: row.summary.key.category, summary: row.summary, members: [...row.members] };
      bundles.set(key, bundle);
      dated.push({ kind: "bundle", bundle, section });
      lastSection = section;
    }
  }

  let current = "";
  const heading = (section: DateSection) => {
    const key = `${section.kind}-${String(section.start)}`;
    if (key === current) return;
    current = key;
    rows.push({ kind: "section", key: `section-${key}-${String(rows.length)}`, section: sectionOfDate(section) });
  };
  for (const entry of dated) {
    if (entry.kind === "message") {
      if (!take(entry.item)) continue;
      heading(entry.section);
      rows.push({ kind: "message", key: `message-${String(entry.item.id)}`, item: entry.item, block: null, bundle: null });
      continue;
    }
    const members = entry.bundle.members.filter((item) => !taken(item) && visible.has(item.id));
    const first = members[0];
    if (!first) continue;
    heading(entry.section);
    if (members.length === 1 && entry.bundle.summary.count <= 1) {
      take(first);
      rows.push({ kind: "message", key: `message-${String(first.id)}`, item: first, block: null, bundle: null });
      continue;
    }
    const open = expanded.has(entry.bundle.key);
    rows.push({ kind: "bundle", key: `bundle-${entry.bundle.key}`, bundle: entry.bundle, expanded: open });
    for (const item of members) {
      if (!take(item) || !open) continue;
      rows.push({ kind: "message", key: `message-${String(item.id)}`, item, block: null, bundle: entry.bundle.key });
    }
  }
  return rows;
}
