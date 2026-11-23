// Full class names, so Tailwind finds them in the source. Decision 0031: soft avatar tints.
const TINTS = [
  "bg-avatar-1 text-avatar-1-text",
  "bg-avatar-2 text-avatar-2-text",
  "bg-avatar-3 text-avatar-3-text",
  "bg-avatar-4 text-avatar-4-text",
  "bg-avatar-5 text-avatar-5-text",
  "bg-avatar-6 text-avatar-6-text",
  "bg-avatar-7 text-avatar-7-text",
  "bg-avatar-8 text-avatar-8-text",
] as const;

/** Number of avatar tints in tokens.css (`--ui-avatar-<n>-bg` and `-text`, n = 1…8). */
export const TINT_COUNT = TINTS.length;

/**
 * The tint index (0…7) of a sender: FNV-1a over the address in lower case, so one sender has the
 * same colour in every list, window and theme, and on both platforms. A sender without address
 * falls back to the name.
 */
export function senderTintIndex(email: string, name?: string | null): number {
  const key = (email.trim() || name?.trim() || "").toLowerCase();
  let hash = 0x811c9dc5;
  for (const char of key) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash % TINTS.length;
}

/** Fill and text classes of a sender's avatar tint. */
export function senderTint(email: string, name?: string | null): string {
  return TINTS[senderTintIndex(email, name)] ?? TINTS[0];
}
