import type {
  Account,
  AccountSettings,
  IpcError,
  OAuthAvailability,
  OAuthPageTexts,
  OAuthProvider,
} from "../../src/services/ipc";

/** Stage 14 (decision 0042): what the fake core answers for sign-in with Microsoft and Google. */
export interface OAuthFakeOptions {
  /** Which sign-ins the build offers (default: none, like a build without client IDs). */
  availability?: Partial<OAuthAvailability>;
  /** How the browser step ends: added (default), an error, or `"wait"` until cancelled. */
  outcome?: "added" | "wait" | IpcError;
}

export interface OAuthFakeState {
  /** `addOauthAccount` calls, in order. */
  readonly added: { settings: AccountSettings; provider: OAuthProvider; texts: OAuthPageTexts }[];
  /** `signInAgain` calls. */
  readonly again: string[];
  readonly cancels: number;
}

export function createOAuthFake(
  options: OAuthFakeOptions,
  deps: { add: (settings: AccountSettings, provider: OAuthProvider) => Account; signedIn: (accountId: string) => void },
) {
  const availability: OAuthAvailability = { microsoft: false, google: false, ...options.availability };
  const added: OAuthFakeState["added"] = [];
  const again: string[] = [];
  let cancels = 0;
  let pending: ((error: IpcError) => void) | null = null;

  function browser<T>(done: () => T): Promise<T> | T {
    const outcome = options.outcome ?? "added";
    if (outcome === "added") return done();
    if (outcome === "wait") {
      return new Promise<T>((_, reject) => {
        pending = reject;
      });
    }
    // Tauri rejects with the serialised error value, not an Error.
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    return Promise.reject(outcome);
  }

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_oauth_availability":
        return { ...availability };
      case "add_oauth_account": {
        const settings = args["settings"] as AccountSettings;
        const provider = args["provider"] as OAuthProvider;
        added.push({ settings, provider, texts: args["texts"] as OAuthPageTexts });
        return browser(() => deps.add(settings, provider));
      }
      case "sign_in_again": {
        const accountId = args["accountId"] as string;
        again.push(accountId);
        return browser(() => {
          deps.signedIn(accountId);
          return null;
        });
      }
      case "cancel_oauth_sign_in": {
        cancels += 1;
        const reject = pending;
        pending = null;
        reject?.({ code: "oauth_cancelled", message: "cancelled", detail: null });
        return reject !== null;
      }
      default:
        return undefined;
    }
  }

  return {
    handle,
    state: {
      added,
      again,
      get cancels() {
        return cancels;
      },
    } satisfies OAuthFakeState,
  };
}
