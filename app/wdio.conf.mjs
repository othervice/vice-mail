// WebdriverIO config for the desktop tests (decision 0005). `pnpm test:e2e` builds the test app
// (`--features e2e`) and runs this; `pnpm test:e2e:run` skips the build. Needs a logged-in GUI
// session (a window opens), so it is not part of scripts/check.sh.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DATA_DIR_ENV, WEBDRIVER_PORT, quitApp } from "./tests/e2e/support/lifecycle.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
// Built by `pnpm build:e2e`: debug profile, embedded assets, the production CSP.
const APP = process.env.E2E_APP ?? path.join(here, "../target/debug/mail-app");

export const config = {
  runner: "local",
  // In this order, one after the other. Every spec file gets a freshly started app on the same
  // data directory: `afterSession` quits the app, and before the next spec the service finds the
  // WebDriver port closed and starts the app again (it logs "unreachable — restarting").
  specs: ["./tests/e2e/frame.e2e.mjs", "./tests/e2e/restart.e2e.mjs"],
  maxInstances: 1,
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": { application: APP },
    },
  ],
  services: [
    [
      "@wdio/tauri-service",
      {
        // The WebDriver server inside the app (tauri-plugin-wdio-webdriver). Stated explicitly so
        // the config means the same on every platform.
        driverProvider: "embedded",
        embeddedPort: WEBDRIVER_PORT,
        captureBackendLogs: true,
        backendLogLevel: "warn",
      },
    ],
  ],
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 120000 },
  waitforTimeout: 10000,
  afterSession: async () => {
    await quitApp();
  },
  onPrepare: () => {
    if (!fs.existsSync(APP)) {
      throw new Error(`test app missing: ${APP}. Build it with \`pnpm --dir app build:e2e\`.`);
    }
    // The test app keeps its database and logs here and never in the user's data directory: e2e
    // builds refuse to start without it. The app and the workers inherit the variable. A
    // directory given from outside is kept for inspection; our own is removed when the run ends
    // (on exit, after the service has stopped the app).
    if (!process.env[DATA_DIR_ENV]) {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mail-app-e2e-"));
      process.env[DATA_DIR_ENV] = dir;
      process.on("exit", () => {
        fs.rmSync(dir, { recursive: true, force: true });
      });
    }
    // WebKit keeps its caches under the Foundation home directory (~/Library/WebKit, ~/Library/Caches);
    // CFFIXED_USER_HOME moves them into the test directory too.
    process.env.CFFIXED_USER_HOME = path.join(process.env[DATA_DIR_ENV], "home");
    fs.mkdirSync(process.env.CFFIXED_USER_HOME, { recursive: true });
    console.log(`test data directory: ${process.env[DATA_DIR_ENV]}`);
  },
};
