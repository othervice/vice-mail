// WebdriverIO config for Spike D. `pnpm test:e2e` builds the test app (`--features e2e`) and runs
// this; `pnpm test:e2e:run` skips the build. The counter server runs in the launcher process.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { startCounter } from "./e2e/counter-server.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
// Built by `pnpm build:e2e` (debug profile, embedded assets, production CSP plus the e2e override).
const APP = process.env.E2E_APP ?? path.join(here, "src-tauri/target/debug/vice-mail-spike");

let counterServer;

export const config = {
  runner: "local",
  specs: ["./e2e/**/*.e2e.mjs"],
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
        // Embedded WebDriver (tauri-plugin-wdio-webdriver inside the app). Auto-detected on macOS;
        // stated explicitly so the config means the same on Windows and Linux.
        driverProvider: "embedded",
        captureBackendLogs: true,
        backendLogLevel: "info",
      },
    ],
  ],
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 240000 },
  waitforTimeout: 20000,
  onPrepare: async () => {
    counterServer = await startCounter({ quiet: true });
  },
  onComplete: async () => {
    await counterServer?.stop();
  },
};
