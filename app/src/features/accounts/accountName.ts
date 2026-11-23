import type { Account, AccountId } from "../../services/ipc";

/**
 * Stage 8 ("Kontofarben immer mit Namen"): the name a row of a list that mixes accounts shows next
 * to its account colour. The display name, unless it is empty or another account has the same
 * one (one person, two addresses): then the address, which tells them apart.
 */
export function accountShortNames(accounts: readonly Account[]): Map<AccountId, string> {
  const counts = new Map<string, number>();
  for (const account of accounts) {
    const name = account.displayName.trim();
    if (name) counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  return new Map(
    accounts.map((account) => {
      const name = account.displayName.trim();
      return [account.id, name && counts.get(name) === 1 ? name : account.email] as const;
    }),
  );
}
