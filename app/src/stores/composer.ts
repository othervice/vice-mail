import { create } from "zustand";
import type { AccountId, Address, Draft, DraftId, IpcError, MailtoNotice, MessageId, OperationId, OutgoingMail } from "../services/ipc";

/** Stage 9: the other fields of a `mailto:` link a new draft starts with, and the link's hints. */
export interface ComposePrefill {
  cc: readonly Address[];
  bcc: readonly Address[];
  subject: string | null;
  /** Plain text with `\n` line breaks. */
  body: string | null;
  /** The message the link answers; a new draft cannot carry it, so the composer says so. */
  inReplyTo: string | null;
  notices: readonly MailtoNotice[];
}

/** What the composer opens with. The composer asks the core for the draft itself. */
export type ComposerSource =
  /** An empty message from the account, optionally to someone already (or filled from a link). */
  | { kind: "new"; accountId: AccountId; to?: readonly Address[]; prefill?: ComposePrefill }
  | { kind: "reply" | "reply_all" | "forward"; messageId: MessageId }
  /** A local draft again: after "Undo" of a send, "Edit" of a failed one, or a restored draft. */
  | { kind: "draft"; draftId: DraftId; draft?: Draft }
  /** A message of the Drafts folder (also one written by another program). */
  | { kind: "message"; messageId: MessageId };

export interface ComposerSession {
  /** New for every opening, so the composer starts fresh even for the same draft. */
  seq: number;
  source: ComposerSource;
}

/**
 * A send the user started in this window, or one the core asks about (`sendNeedsDecision`):
 * what the send notices show. Kept as the core's `OutgoingMail`, translated by the component.
 */
export interface TrackedSend {
  mail: OutgoingMail;
  /** The user confirmed "Send again" is what they want (the second step of that choice). */
  confirmingSendAgain: boolean;
}

/** A step outside the open composer failed (closing, undo, an answer in a notice). */
export interface ComposerProblem {
  id: number;
  context: "close" | "undo" | "edit" | "resolve" | "retry" | "restore";
  error: IpcError;
}

interface ComposerState {
  session: ComposerSession | null;
  /** The draft the open composer edits, once it has one (for `draftsChanged`). */
  openDraftId: DraftId | null;
  /** Sends shown as notices, oldest first. */
  sends: readonly TrackedSend[];
  open: (source: ComposerSource) => void;
  /** Closes the composer. It saves and hands the draft to the server copy itself on unmount. */
  close: () => void;
  setOpenDraft: (draftId: DraftId | null) => void;
  /** A send the user just started. */
  track: (mail: OutgoingMail) => void;
  /**
   * New states from `outgoingChanged`. Only sends already shown change; a send that failed or
   * became unclear is shown even if this window did not start it (it needs the user).
   */
  update: (mails: readonly OutgoingMail[]) => void;
  /** `sendNeedsDecision`: the question is shown until the user answers. */
  needsDecision: (mail: OutgoingMail) => void;
  setConfirmingSendAgain: (operationId: OperationId, confirming: boolean) => void;
  dismiss: (operationId: OperationId) => void;
  problems: readonly ComposerProblem[];
  /** The offer to reopen drafts left from the last session was taken or declined. */
  restoreDone: boolean;
  finishRestore: () => void;
  report: (context: ComposerProblem["context"], error: IpcError) => void;
  dismissProblem: (id: number) => void;
}

/** At most this many send notices; the oldest finished one goes first. */
const MAX_SENDS = 4;

let nextSeq = 1;
let nextProblem = 1;

function upsert(sends: readonly TrackedSend[], mail: OutgoingMail): TrackedSend[] {
  const index = sends.findIndex((s) => s.mail.operationId === mail.operationId);
  if (index < 0) return [...sends, { mail, confirmingSendAgain: false }];
  const next = [...sends];
  const current = next[index];
  if (current) next[index] = { ...current, mail };
  return next;
}

function trim(sends: TrackedSend[]): TrackedSend[] {
  const next = [...sends];
  while (next.length > MAX_SENDS) {
    // Finished ones go first; a question for the user stays.
    const index = next.findIndex((s) => s.mail.state === "sent");
    next.splice(index >= 0 ? index : 0, 1);
  }
  return next;
}

export const useComposerStore = create<ComposerState>()((set) => ({
  session: null,
  openDraftId: null,
  sends: [],
  open: (source) => {
    set({
      session: { seq: nextSeq++, source },
      openDraftId: source.kind === "draft" ? source.draftId : null,
      restoreDone: true,
    });
  },
  close: () => {
    set({ session: null, openDraftId: null });
  },
  setOpenDraft: (draftId) => {
    set({ openDraftId: draftId });
  },
  track: (mail) => {
    set((state) => ({ sends: trim(upsert(state.sends, mail)) }));
  },
  update: (mails) => {
    set((state) => {
      let sends = [...state.sends];
      for (const mail of mails) {
        const known = sends.some((s) => s.mail.operationId === mail.operationId);
        if (known || mail.state === "failed" || mail.state === "unclear") sends = upsert(sends, mail);
      }
      return { sends: trim(sends) };
    });
  },
  needsDecision: (mail) => {
    set((state) => ({ sends: trim(upsert(state.sends, mail)) }));
  },
  setConfirmingSendAgain: (operationId, confirming) => {
    set((state) => ({
      sends: state.sends.map((s) => (s.mail.operationId === operationId ? { ...s, confirmingSendAgain: confirming } : s)),
    }));
  },
  dismiss: (operationId) => {
    set((state) => ({ sends: state.sends.filter((s) => s.mail.operationId !== operationId) }));
  },
  problems: [],
  restoreDone: false,
  finishRestore: () => {
    set({ restoreDone: true });
  },
  report: (context, error) => {
    set((state) => ({ problems: [...state.problems, { id: nextProblem++, context, error }].slice(-3) }));
  },
  dismissProblem: (id) => {
    set((state) => ({ problems: state.problems.filter((p) => p.id !== id) }));
  },
}));

/** Where the focus was before the composer opened; it goes back there when the composer closes. */
let returnFocus: HTMLElement | null = null;

/** Opens the composer from anywhere (actions, header button, outbox view). */
export function openComposer(source: ComposerSource): void {
  const active = document.activeElement;
  // Not from inside the composer itself (Undo opens a new one while the old focus is gone anyway).
  if (active instanceof HTMLElement && !active.closest('[data-testid="composer"]')) returnFocus = active;
  useComposerStore.getState().open(source);
}

/** The element to focus after the composer closed, if it is still on screen. */
export function takeReturnFocus(): HTMLElement | null {
  const target = returnFocus;
  returnFocus = null;
  return target?.isConnected ? target : null;
}
