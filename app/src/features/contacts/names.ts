import type { ContactEntry } from "../../services/ipc";

/** The name to show, or the "(no name)" text. */
export function contactName(contact: ContactEntry, noName: string): string {
  return contact.name?.trim() ? contact.name : noName;
}
