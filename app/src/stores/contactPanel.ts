import { create } from "zustand";
import type { AccountId } from "../services/ipc";

/** Whom the contact panel shows: the other side of the message in the reading pane. */
export interface PanelPerson {
  email: string;
  /** The name the message gives, for a sender who is no contact yet. */
  name: string | null;
  /** The account of the message: "Write mail" writes from it. */
  accountId: AccountId;
}

interface ContactPanelState {
  person: PanelPerson | null;
  /** The reading pane shows a message with this person (null: nothing shown). */
  show: (person: PanelPerson | null) => void;
  /** The pane that showed `email` is gone; a newer pane may have set another person already. */
  leave: (email: string) => void;
}

/**
 * Stage 23, step 3 (decision 0084): the person of the message in the reading pane, for the
 * contact panel on the right. Set by the reading pane of the main window; the panel reads the
 * contact itself from the contacts store.
 */
export const useContactPanelStore = create<ContactPanelState>()((set, get) => ({
  person: null,
  show: (person) => {
    const current = get().person;
    if (current?.email === person?.email && current?.accountId === person?.accountId && current?.name === person?.name) return;
    set({ person });
  },
  leave: (email) => {
    if (get().person?.email === email) set({ person: null });
  },
}));
