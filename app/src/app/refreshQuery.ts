import { hashKey, type QueryClient, type QueryKey } from "@tanstack/react-query";

/** Events of one burst (the engine sends `messagesChanged` and `mailboxesChanged` together). */
const BURST_MS = 16;

interface RefreshState {
  timer: ReturnType<typeof setTimeout> | null;
  running: boolean;
  /** Another event arrived while a reload was running: reload once more afterwards. */
  again: boolean;
}

const states = new WeakMap<QueryClient, Map<string, RefreshState>>();

function stateOf(queryClient: QueryClient, key: string): RefreshState {
  let byKey = states.get(queryClient);
  if (!byKey) {
    byKey = new Map();
    states.set(queryClient, byKey);
  }
  let state = byKey.get(key);
  if (!state) {
    state = { timer: null, running: false, again: false };
    byKey.set(key, state);
  }
  return state;
}

/**
 * Reloads one query after a core event, coalescing bursts. At most one reload runs at a time;
 * events during it lead to exactly one more reload when it ends. So a stream of events during a
 * long first sync neither restarts a reload over and over (it would never finish) nor gets
 * lost. A query that is not on screen is only marked stale and reloads when it is shown again.
 *
 * A first answer still on its way when the reload is due may have been read before what the
 * event announces, and it can reach the page after the event. TanStack Query does not ask again
 * then: a reload of a query without data hands back the ask on its way (only an ask of a query
 * that has data is cancelled and started anew). So the reload waits for that answer and asks once
 * more, unless `stillWaits` says the answer already has what the event announced.
 */
export function refreshQuery<T = unknown>(queryClient: QueryClient, queryKey: QueryKey, stillWaits?: (data: T | undefined) => boolean): void {
  const state = stateOf(queryClient, hashKey(queryKey));
  if (state.running || state.timer !== null) {
    if (state.running) state.again = true;
    return;
  }
  state.timer = setTimeout(() => {
    state.timer = null;
    state.running = true;
    state.again = false;
    void reload(queryClient, queryKey, stillWaits).finally(() => {
      state.running = false;
      if (state.again) refreshQuery(queryClient, queryKey, stillWaits);
    });
  }, BURST_MS);
}

async function reload<T>(queryClient: QueryClient, queryKey: QueryKey, stillWaits?: (data: T | undefined) => boolean): Promise<void> {
  const query = queryClient.getQueryCache().find({ queryKey, exact: true });
  const onItsWay = query && query.state.data === undefined ? query.promise : undefined;
  if (onItsWay) {
    // A failed ask is asked again below, like any query that shows an error.
    await onItsWay.catch(() => undefined);
    if (stillWaits && !stillWaits(queryClient.getQueryData<T>(queryKey))) return;
  }
  // Never rejects: a failed reload ends up as the query's error, which the view shows.
  await queryClient.invalidateQueries({ queryKey, exact: true });
}
