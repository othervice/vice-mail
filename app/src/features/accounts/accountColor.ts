import type { AccountColor } from "../../services/ipc";

// Full class names, so Tailwind finds them in the source. Index = `Account.color` (0–7).
const RING = [
  "ring-account-1",
  "ring-account-2",
  "ring-account-3",
  "ring-account-4",
  "ring-account-5",
  "ring-account-6",
  "ring-account-7",
  "ring-account-8",
] as const;

const FILL = [
  "bg-account-1",
  "bg-account-2",
  "bg-account-3",
  "bg-account-4",
  "bg-account-5",
  "bg-account-6",
  "bg-account-7",
  "bg-account-8",
] as const;

/** The core sends 0–7; anything else (a newer core with more colours) wraps around. */
function index(color: AccountColor): number {
  return ((Math.trunc(color) % RING.length) + RING.length) % RING.length;
}

/** Ring in the account colour, e.g. around avatars. Always shown together with the account name. */
export function accountRing(color: AccountColor): string {
  return RING[index(color)] ?? RING[0];
}

/** Filled dot in the account colour. Always shown together with the account name. */
export function accountFill(color: AccountColor): string {
  return FILL[index(color)] ?? FILL[0];
}

/** Up to two initials of a name or address, for avatars ("Ada Lovelace" → "AL", "ops@x" → "O"). */
export function initials(name: string | null | undefined, email: string): string {
  const source = name?.trim() || email.split("@")[0] || email;
  const words = source
    .replace(/["'()<>[\]]/g, "")
    .split(/[\s._-]+/)
    .filter(Boolean);
  const letters = words.length > 1 ? [words[0], words[words.length - 1]] : [words[0]];
  const result = letters
    .map((word) => (word ? Array.from(word)[0] ?? "" : ""))
    .join("")
    .toLocaleUpperCase();
  return result || "?";
}
