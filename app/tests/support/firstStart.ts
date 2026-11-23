import type { Account, FirstStart, FirstStartEnd } from "../../src/services/ipc";

/** Stage 15 (decision 0044): what the fake core answers for the guided first start. */
export interface FirstStartFakeOptions {
  /**
   * A new installation: the first start never ended. Like the core, it then shows only while
   * there is no account (an installation with one counts as done, `notNeeded`). Default: ended,
   * so the other tests see the frame.
   */
  pending?: boolean;
}

export interface FirstStartFakeState {
  readonly state: FirstStart;
  /** `finishFirstStart` calls, in order. */
  readonly ends: FirstStartEnd[];
}

export function createFirstStartFake(options: FirstStartFakeOptions, deps: { accounts: () => Account[] }) {
  let state: FirstStart = options.pending ? { show: true, outcome: null } : { show: false, outcome: "completed" };
  const ends: FirstStartEnd[] = [];

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_first_start":
        if (state.outcome === null && deps.accounts().length > 0) state = { show: false, outcome: "notNeeded" };
        return { ...state };
      case "finish_first_start": {
        const end = args["end"] as FirstStartEnd;
        ends.push(end);
        state = { show: false, outcome: end };
        return { ...state };
      }
      default:
        return undefined;
    }
  }

  return {
    handle,
    state: {
      get state() {
        return state;
      },
      ends,
    } satisfies FirstStartFakeState,
  };
}
