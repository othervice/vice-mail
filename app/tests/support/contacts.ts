// Stage 10: the fake core's contact directory (ranked list, names, duplicates, merge, separate),
// with the command names and argument shapes of the stage 10 IPC contract (decision 0023).
import type { ContactDuplicate, ContactEntry, ContactMergeRequest, IpcError } from "../../src/services/ipc";

export function contactsError(code: IpcError["code"], detail: string | null = null): IpcError {
  return { code, message: `English fallback for ${code}`, detail };
}

/** A contact with one address; tests pass everything that differs. */
export function makeContact(overrides: Partial<ContactEntry> & Pick<ContactEntry, "email">): ContactEntry {
  const sent = overrides.sent ?? 0;
  const received = overrides.received ?? 0;
  const lastUsed = overrides.lastUsed ?? null;
  return {
    key: overrides.email.toLowerCase(),
    name: null,
    userName: null,
    addresses: [{ email: overrides.email, sent, received, lastUsed }],
    sent,
    received,
    lastUsed,
    ...overrides,
  };
}

export interface ContactsFakeOptions {
  /** The directory, highest ranked first (the core's order). */
  contacts?: ContactEntry[];
  duplicates?: ContactDuplicate[];
}

export interface ContactsFakeState {
  /** The directory as the fake core holds it now, ranked. */
  readonly contacts: readonly ContactEntry[];
  /** Merges the core carried out (only confirmed ones). */
  readonly merges: readonly ContactMergeRequest[];
  /** Duplicate suggestions the user dismissed (contact keys). */
  readonly dismissed: readonly (readonly string[])[];
  /** Addresses taken out of a merged contact. */
  readonly separated: readonly string[];
}

const PAGE_LIMIT = 200;

export function createContactsFake(options: ContactsFakeOptions) {
  let contacts: ContactEntry[] = (options.contacts ?? []).map((c) => ({ ...c, addresses: [...c.addresses] }));
  let duplicates: ContactDuplicate[] = [...(options.duplicates ?? [])];
  // The name seen in mail, to go back to when the user clears theirs.
  const seenNames = new Map(contacts.map((c) => [c.key, c.userName === null ? c.name : null]));
  const merges: ContactMergeRequest[] = [];
  const dismissed: string[][] = [];
  const separated: string[] = [];

  const byAddress = (email: string): ContactEntry | undefined => {
    const wanted = email.trim().toLowerCase();
    return contacts.find((c) => c.addresses.some((a) => a.email.toLowerCase() === wanted) || c.key === wanted);
  };
  const reject = (code: IpcError["code"], detail: string | null = null) =>
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    Promise.reject(contactsError(code, detail));
  const replace = (next: ContactEntry) => {
    contacts = contacts.map((c) => (c.key === next.key ? next : c));
    duplicates = duplicates.map((d) => ({ ...d, contacts: d.contacts.map((c) => (c.key === next.key ? next : c)) }));
  };

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "list_contacts": {
        const limit = args["limit"] as number;
        if (!Number.isInteger(limit) || limit < 1 || limit > PAGE_LIMIT) return reject("invalid_argument", "limit");
        const words = ((args["query"] as string | null) ?? "").toLowerCase().split(/\s+/).filter(Boolean);
        const matching = contacts.filter((c) => {
          const haystack = [c.name ?? "", ...c.addresses.map((a) => a.email)].join(" ").toLowerCase();
          return words.every((w) => haystack.includes(w));
        });
        const start = Number((args["cursor"] as string | null) ?? "0");
        const end = start + limit;
        return { items: matching.slice(start, end), next: end < matching.length ? String(end) : null };
      }
      case "get_contact": {
        const found = byAddress(args["email"] as string);
        return found ? { ...found } : reject("not_found");
      }
      case "set_contact_name": {
        const found = byAddress(args["email"] as string);
        if (!found) return reject("not_found");
        const raw = args["name"] as string | null;
        const userName = raw?.trim() ? raw.trim() : null;
        const next: ContactEntry = { ...found, userName, name: userName ?? seenNames.get(found.key) ?? null };
        replace(next);
        return next;
      }
      case "list_contact_duplicates":
        return duplicates.map((d) => ({ ...d }));
      case "dismiss_contact_duplicate": {
        const keys = [...(args["contacts"] as string[])].sort();
        dismissed.push(keys);
        duplicates = duplicates.filter((d) => d.contacts.map((c) => c.key).sort().join(" ") !== keys.join(" "));
        return null;
      }
      case "merge_contacts": {
        const request = args["request"] as ContactMergeRequest;
        if (request.confirmed !== true) return reject("confirmation_required");
        const keep = byAddress(request.keep);
        const others = request.merge.map(byAddress);
        if (!keep || others.some((o) => !o)) return reject("not_found");
        const merged = others.filter((o): o is ContactEntry => o !== undefined && o.key !== keep.key);
        const next: ContactEntry = {
          ...keep,
          addresses: [...keep.addresses, ...merged.flatMap((m) => m.addresses)],
          sent: keep.sent + merged.reduce((n, m) => n + m.sent, 0),
          received: keep.received + merged.reduce((n, m) => n + m.received, 0),
        };
        const gone = new Set(merged.map((m) => m.key));
        contacts = contacts.filter((c) => !gone.has(c.key));
        replace(next);
        duplicates = duplicates.filter((d) => !d.contacts.some((c) => gone.has(c.key) || c.key === keep.key));
        merges.push(request);
        return next;
      }
      case "separate_contact": {
        const email = (args["email"] as string).toLowerCase();
        const found = byAddress(email);
        if (!found) return reject("not_found");
        const address = found.addresses.find((a) => a.email.toLowerCase() === email);
        if (!address || found.addresses.length < 2 || found.addresses[0] === address) {
          return reject("invalid_argument", "not a merged address");
        }
        const rest = found.addresses.filter((a) => a !== address);
        const kept: ContactEntry = {
          ...found,
          addresses: rest,
          sent: found.sent - address.sent,
          received: found.received - address.received,
        };
        const alone: ContactEntry = {
          key: email,
          name: found.name,
          userName: null,
          email: address.email,
          addresses: [address],
          sent: address.sent,
          received: address.received,
          lastUsed: address.lastUsed,
        };
        replace(kept);
        const index = contacts.findIndex((c) => c.key === kept.key);
        contacts = [...contacts.slice(0, index + 1), alone, ...contacts.slice(index + 1)];
        seenNames.set(alone.key, found.name);
        separated.push(address.email);
        return alone;
      }
      default:
        return undefined;
    }
  }

  const state: ContactsFakeState = {
    get contacts() {
      return contacts;
    },
    get merges() {
      return merges;
    },
    get dismissed() {
      return dismissed;
    },
    get separated() {
      return separated;
    },
  };
  return { handle, state };
}
