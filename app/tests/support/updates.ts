import type { UpdateInfo, UpdateStatus } from "../../src/services/ipc";

/** Stage 15 (decision 0037): what the fake core answers for the update commands. */
export interface UpdatesFakeOptions {
  /** Default: a build without the updater (About shows only the version, nothing is asked). */
  info?: Partial<UpdateInfo>;
  /** What "Check now" finds; default: up to date. */
  checkResult?: UpdateStatus;
}

export interface UpdatesFakeState {
  readonly info: UpdateInfo;
  /** Sets the state, as the core would before emitting `updateStatusChanged`. */
  setStatus: (status: UpdateStatus) => void;
}

const UNSUPPORTED: UpdateInfo = {
  currentVersion: "0.1.0",
  supported: false,
  autoCheck: false,
  askConsent: false,
  lastCheckMs: null,
  status: { state: "idle" },
};

export function createUpdatesFake(options: UpdatesFakeOptions, deps: { reject: (code: "update_not_ready") => unknown }) {
  let info: UpdateInfo = { ...UNSUPPORTED, ...options.info };
  const answer = (): UpdateInfo => ({ ...info });

  function handle(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_update_info":
        return answer();
      case "set_update_auto_check":
        info = { ...info, autoCheck: args["enabled"] === true, askConsent: false };
        return answer();
      case "check_for_updates":
        info = {
          ...info,
          lastCheckMs: Date.now(),
          status: options.checkResult ?? { state: "upToDate", checkedAtMs: Date.now() },
        };
        return answer();
      case "download_update": {
        const status = info.status;
        const version =
          status.state === "available" ? status.update.version : status.state === "failed" ? status.version : null;
        if (version === null) return deps.reject("update_not_ready");
        info = { ...info, status: { state: "downloading", version, downloaded: 0, total: null } };
        return answer();
      }
      case "install_update": {
        if (info.status.state !== "ready") return deps.reject("update_not_ready");
        info = { ...info, status: { state: "installing", version: info.status.version } };
        return answer();
      }
      default:
        return undefined;
    }
  }

  const state: UpdatesFakeState = {
    get info() {
      return info;
    },
    setStatus: (status) => {
      info = { ...info, status };
    },
  };
  return { handle, state };
}
