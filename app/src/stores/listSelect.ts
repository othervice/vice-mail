import { useEffect, useLayoutEffect, useRef } from "react";
import { create } from "zustand";
import type { AccountId, MailboxId, MessageId } from "../services/ipc";

/**
 * Stage 9 (decision 0028): the message list of the main window selects a message by id, so a
 * click on a system notification or on a source of an AI answer shows the mail where the user
 * reads mail, instead of in a window of its own. Only what the list has loaded counts: it is
 * virtualized and paged, and the message may be on no loaded page (then the window opens).
 */

/** Which list answers: a folder of one account, or a list that mixes accounts (`accountId` null). */
export interface ListSelectScope {
  accountId: AccountId | null;
  mailboxId: MailboxId | null;
  /** The account's inbox (a notification without a folder means the inbox). */
  inbox: boolean;
}

/** Lists that mix accounts or folders (unified inbox, search): only immediate requests. */
export const MIXED_LIST_SCOPE: ListSelectScope = { accountId: null, mailboxId: null, inbox: false };

/** A list that can select messages by id. */
export interface ListSelectAnswerer {
  scope: ListSelectScope;
  /** Its first page is there (rows to look in). */
  loaded: () => boolean;
  /** Loaded and no fetch runs (a reload may bring the message). */
  settled: () => boolean;
  /** Selects the message, scrolls it into view and opens it; false if the list does not have it. */
  select: (messageId: MessageId) => boolean;
}

/** A message to select once the list of its folder shows (the view changes in the same tick). */
export interface ListSelectRequest {
  accountId: AccountId;
  /** Null: the account's inbox. */
  mailboxId: MailboxId | null;
  messageId: MessageId;
  /** The list does not have it, or no list of the folder showed in time. */
  fallback: () => void;
}

interface Pending extends ListSelectRequest {
  seq: number;
}

/** How long a request waits for the list of its folder. Afterwards the fallback runs. */
export const LIST_SELECT_WAIT_MS = 5_000;

interface ListSelectState {
  /** Mounted lists, the most recently mounted last (the search results lie over a folder). */
  lists: readonly { key: string; answerer: ListSelectAnswerer }[];
  pending: Pending | null;
  register: (key: string, answerer: ListSelectAnswerer) => void;
  unregister: (key: string) => void;
}

let timer: ReturnType<typeof setTimeout> | null = null;
let seq = 0;
let listKeys = 0;

function clearTimer(): void {
  if (timer !== null) clearTimeout(timer);
  timer = null;
}

export const useListSelectStore = create<ListSelectState>()((set) => ({
  lists: [],
  pending: null,
  register: (key, answerer) => {
    set((state) => ({ lists: [...state.lists.filter((entry) => entry.key !== key), { key, answerer }] }));
  },
  unregister: (key) => {
    set((state) => ({ lists: state.lists.filter((entry) => entry.key !== key) }));
  },
}));

export function scopeMatches(scope: ListSelectScope, request: Pick<ListSelectRequest, "accountId" | "mailboxId">): boolean {
  if (scope.accountId === null || scope.accountId !== request.accountId) return false;
  return request.mailboxId === null ? scope.inbox : scope.mailboxId === request.mailboxId;
}

/** Takes the pending request if it is still `pending` (one list answers it, once). */
function take(pending: Pending): boolean {
  if (useListSelectStore.getState().pending?.seq !== pending.seq) return false;
  clearTimer();
  useListSelectStore.setState({ pending: null });
  return true;
}

/**
 * Selects `messageId` in the list the main window shows now (the most recently mounted one), if
 * that list has loaded it. False: no list, or not among its loaded rows.
 */
export function selectInList(messageId: MessageId): boolean {
  const { lists } = useListSelectStore.getState();
  const current = lists[lists.length - 1]?.answerer;
  return Boolean(current?.loaded() && current.select(messageId));
}

/**
 * Selects a message in the list of its folder: at once when that list shows and is loaded,
 * otherwise as soon as it is (at most `LIST_SELECT_WAIT_MS`). The fallback runs when the list
 * does not have the message or did not show in time. A newer request replaces an older one.
 */
export function selectWhenListed(request: ListSelectRequest): void {
  clearTimer();
  seq += 1;
  const pending: Pending = { ...request, seq };
  useListSelectStore.setState({ pending });
  const lists = [...useListSelectStore.getState().lists].reverse();
  const ready = lists.find((entry) => scopeMatches(entry.answerer.scope, request) && entry.answerer.settled());
  if (ready) {
    take(pending);
    if (!ready.answerer.select(request.messageId)) request.fallback();
    return;
  }
  timer = setTimeout(() => {
    if (take(pending)) request.fallback();
  }, LIST_SELECT_WAIT_MS);
}

/**
 * The list's side: registers it while mounted and answers a waiting request for its scope once
 * its rows are there and no fetch runs (a fresh mount may show cached pages while it reloads).
 * `select` is read at the moment of the call, so it always sees the current rows.
 */
export function useListSelectAnswer(
  scope: ListSelectScope | undefined,
  loaded: boolean,
  settled: boolean,
  select: (messageId: MessageId) => boolean,
): void {
  const selectRef = useRef(select);
  const loadedRef = useRef(loaded);
  const settledRef = useRef(settled);
  useLayoutEffect(() => {
    selectRef.current = select;
    loadedRef.current = loaded;
    settledRef.current = loaded && settled;
  });
  const accountId = scope?.accountId ?? null;
  const mailboxId = scope?.mailboxId ?? null;
  const inbox = scope?.inbox ?? false;
  const enabled = scope !== undefined;
  useEffect(() => {
    if (!enabled) return undefined;
    listKeys += 1;
    const key = `list-${String(listKeys)}`;
    useListSelectStore.getState().register(key, {
      scope: { accountId, mailboxId, inbox },
      loaded: () => loadedRef.current,
      settled: () => settledRef.current,
      select: (messageId) => selectRef.current(messageId),
    });
    return () => {
      useListSelectStore.getState().unregister(key);
    };
  }, [enabled, accountId, mailboxId, inbox]);

  const pending = useListSelectStore((state) => state.pending);
  useEffect(() => {
    if (!enabled || !pending || !loaded || !settled) return;
    if (!scopeMatches({ accountId, mailboxId, inbox }, pending)) return;
    if (!take(pending)) return;
    if (!selectRef.current(pending.messageId)) pending.fallback();
  }, [enabled, pending, loaded, settled, accountId, mailboxId, inbox]);
}

/** Tests: forgets lists, a waiting request and its timer. */
export function resetListSelectForTests(): void {
  clearTimer();
  useListSelectStore.setState(useListSelectStore.getInitialState(), true);
}
