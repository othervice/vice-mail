import type { MessageListItem } from "../../services/ipc";

/** Longest keyword the core accepts, in bytes (all ASCII, so characters). */
const MAX_KEYWORD_LENGTH = 128;

/** IMAP atom-specials besides space and control characters (RFC 3501): `( ) { ] % * " \`. */
const SPECIALS = new Set(["(", ")", "{", "]", "%", "*", '"', "\\"]);

/**
 * Whether the core accepts `keyword` as a tag (it checks the same rules and answers
 * `invalid_keyword`): printable ASCII without space and IMAP specials, 1 to 128 characters.
 * Checking here explains the problem at the field before anything is sent.
 */
export function isValidKeyword(keyword: string): boolean {
  if (keyword.length === 0 || keyword.length > MAX_KEYWORD_LENGTH) return false;
  for (const char of keyword) {
    const code = char.codePointAt(0) ?? 0;
    if (code <= 0x20 || code >= 0x7f || SPECIALS.has(char)) return false;
  }
  return true;
}

/**
 * Keywords the app shows as tags. Those starting with `$` and a few well-known ones are flags
 * other programs set for themselves ($Forwarded, $MDNSent, $Junk, NonJunk, …), not the user's tags.
 */
const SYSTEM_KEYWORDS = new Set(["junk", "nonjunk", "notjunk", "old"]);

export function isUserTag(keyword: string): boolean {
  return !keyword.startsWith("$") && !SYSTEM_KEYWORDS.has(keyword.toLowerCase());
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/** The tags of some messages, each once (case as first seen), sorted by name. */
export function tagsOf(items: readonly MessageListItem[]): string[] {
  const seen = new Map<string, string>();
  for (const item of items) {
    for (const keyword of item.keywords) {
      if (isUserTag(keyword) && !seen.has(keyword.toLowerCase())) seen.set(keyword.toLowerCase(), keyword);
    }
  }
  return [...seen.values()].sort((a, b) => collator.compare(a, b));
}

export function hasTag(item: MessageListItem, tag: string): boolean {
  const lower = tag.toLowerCase();
  return item.keywords.some((keyword) => keyword.toLowerCase() === lower);
}
