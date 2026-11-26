import type { AccountColor } from "../../services/ipc";

// Full class names, so Tailwind finds them in the source. Index = `Account.color` (0–7).
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

// Stage 23 (decision 0085): the account's colour as the colour of its icons in the sidebar.
const TEXT = [
  "text-account-1",
  "text-account-2",
  "text-account-3",
  "text-account-4",
  "text-account-5",
  "text-account-6",
  "text-account-7",
  "text-account-8",
] as const;

/** The core sends 0–7; anything else (a newer core with more colours) wraps around. */
function index(color: AccountColor): number {
  return ((Math.trunc(color) % FILL.length) + FILL.length) % FILL.length;
}

/** Filled dot in the account colour. Always shown together with the account name. */
export function accountFill(color: AccountColor): string {
  return FILL[index(color)] ?? FILL[0];
}

/**
 * Stage 23 (decision 0085): icons in the account colour (its folders in the sidebar). Always
 * next to a name that says the same (the folder's, under the account's heading or in its label).
 */
export function accountText(color: AccountColor): string {
  return TEXT[index(color)] ?? TEXT[0];
}
