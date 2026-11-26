// WebdriverIO config for the desktop tests (decision 0005). `pnpm test:e2e` builds the test app
// (`--features e2e`) and runs this; `pnpm test:e2e:run` skips the build. Needs a logged-in GUI
// session (a window opens), so it is not part of scripts/check.sh.
//
// The run owns the local test server (scripts/testmail.sh): it resets it, starts it and seeds
// 10,000 messages before the first app starts, and stops it at the end. `reset` creates a new
// test CA, and the app reads the CA once at start, so this has to happen here and not in a spec.
// Other sessions share that server: the run takes the shared lock first (`testmail.sh lock`, name
// from TESTMAIL_LOCK_NAME, default `desktop-e2e`) and releases it at the end, unless the caller
// already held it under that name, then the caller keeps it.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DATA_DIR_ENV, WEBDRIVER_PORT, quitApp } from "./tests/e2e/support/lifecycle.mjs";
import { SevereServiceError } from "webdriverio";
import { CA_FILE, EXTRA_CA_ENV, LOOPBACK_IMAGES_ENV, prepareTestServer, removeLeftoverVault, testmailSync } from "./tests/e2e/support/testmail.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
// Built by `pnpm build:e2e`: debug profile, embedded assets, the production CSP.
const APP = process.env.E2E_APP ?? path.join(here, "../target/debug/mail-app");
const LOCK_NAME = process.env.TESTMAIL_LOCK_NAME || "desktop-e2e";
// README_SHOTS=1: the README screenshots (tests/e2e/readme-shots.e2e.mjs, docs/images/README.md),
// taken with `--spec`. They need the seed of the screenshot rules (1,000 messages, without the
// SMTP test messages) instead of the 10,000 the other specs use; without the variable the spec is
// excluded, even when named.
const README_SHOTS = process.env.README_SHOTS === "1";
// PERF=1: the performance measurement (tests/e2e/perf.e2e.mjs, docs/testing/performance.md), taken
// with `--spec`, on a seed of PERF_SEED messages (default 10,000) into alice's INBOX. Without the
// variable the spec is excluded, even when named.
const PERF = process.env.PERF === "1";
const PERF_SEED = process.env.PERF_SEED || "10000";
/** Whether this run took the lock (and so releases it); false if the caller held it already. */
let ownLock = false;

export const config = {
  runner: "local",
  // In this order, one after the other. Every spec file gets a freshly started app on the same
  // data directory: `afterSession` quits the app, and before the next spec the service finds the
  // WebDriver port closed and starts the app again (it logs "unreachable — restarting").
  // account-sync.e2e.mjs needs the empty data directory the first three leave behind, and the
  // server exactly as seeded; reading.e2e.mjs adds folders to it. comfort.e2e.mjs,
  // search.e2e.mjs and conversations.e2e.mjs (the offsite thread in Archive) and extras.e2e.mjs
  // read the seed;
  // keyboard.e2e.mjs archives a message it delivered itself; windows.e2e.mjs only reads.
  // actions.e2e.mjs changes the inbox on the server (archive, trash); compose.e2e.mjs,
  // composer-complete.e2e.mjs and send-later.e2e.mjs write drafts and send to alice.
  // ai.e2e.mjs uses the counting server as its provider; a11y.e2e.mjs runs axe over the main
  // views. layouts.e2e.mjs changes the stored window layout (and restores it) and comes last.
  // first-start.e2e.mjs needs the fresh data directory (the guided first start shows only there)
  // and comes first; every other spec skips the first start in `attach()` if it shows.
  specs: [
    // Stage 15 (decision 0044): the guided first start.
    "./tests/e2e/first-start.e2e.mjs",
    "./tests/e2e/frame.e2e.mjs",
    "./tests/e2e/restart.e2e.mjs",
    "./tests/e2e/account-sync.e2e.mjs",
    "./tests/e2e/reading.e2e.mjs",
    // Stage 16: find in the open message.
    "./tests/e2e/find-in-message.e2e.mjs",
    "./tests/e2e/attachment-preview.e2e.mjs",
    "./tests/e2e/comfort.e2e.mjs",
    "./tests/e2e/search.e2e.mjs",
    // Stage 17 (decision 0049): conversations unfold in the list (the seed's offsite thread).
    "./tests/e2e/conversations.e2e.mjs",
    // Stage 18: the border between list and reading pane, unfolding beside the quick actions
    // (real mouse events). Changes the stored layout and puts it back.
    "./tests/e2e/columns.e2e.mjs",
    // Stage 18, task F (decision 0054): a folder opens with its top message, unread. Marks it
    // read once and unread again.
    "./tests/e2e/top-message.e2e.mjs",
    // Stage 19, task A (decisions 0061, 0062): sorting, the table and the quick filter on the
    // seed's inbox. Tags three messages for a moment and removes the tag again.
    "./tests/e2e/list-views.e2e.mjs",
    "./tests/e2e/extras.e2e.mjs",
    "./tests/e2e/keyboard.e2e.mjs",
    "./tests/e2e/windows.e2e.mjs",
    "./tests/e2e/actions.e2e.mjs",
    "./tests/e2e/compose.e2e.mjs",
    "./tests/e2e/composer-complete.e2e.mjs",
    "./tests/e2e/send-later.e2e.mjs",
    // Stage 16: the inline quick reply.
    "./tests/e2e/quick-reply.e2e.mjs",
    "./tests/e2e/mcp-sync.e2e.mjs",
    "./tests/e2e/ai.e2e.mjs",
    "./tests/e2e/a11y.e2e.mjs",
    // Stage 17, block B (decision 0050): contacts: search, a dropped picture, spam and undo. It
    // appends three messages to the inbox and moves two to Junk and back, after the specs that
    // count the seed.
    "./tests/e2e/contacts.e2e.mjs",
    // Stage 22 (decision 0074): contacts in the search and search inside words. Appends four
    // messages to the inbox; sets up and removes alice itself.
    "./tests/e2e/search-contacts.e2e.mjs",
    // Stage 18, block C (decision 0051): the new-mail popup. It delivers messages of its own,
    // deletes one into the trash and turns writes on for alice, then removes the account.
    "./tests/e2e/new-mail-popup.e2e.mjs",
    // Stage 21: closing the main window keeps the app running (after the popup spec).
    "./tests/e2e/background.e2e.mjs",
    // Stage 18, task D: folders created in the app on the server, and the server's folders
    // reloaded without a restart. Creates folders on the server and removes them again.
    "./tests/e2e/folders.e2e.mjs",
    // Stage 21 (decision 0063): "Mark folder as read" in a folder of its own (removed again), and
    // the column's menus at 200 px in German and English. Sets up and removes alice itself.
    "./tests/e2e/mark-folder-read.e2e.mjs",
    // Stage 20 (decisions 0058 to 0060): attachment reminder, reminders without a reply (it
    // moves the app's clock forward), the attachments view, trackers and the quota (it sets and
    // clears a storage limit for alice). Appends three messages to the inbox.
    "./tests/e2e/stage20.e2e.mjs",
    // Stage 21, items 8 and 9 (decisions 0066, 0067): mails in the dark theme and colours in sent
    // mail. Appends a newsletter to the inbox and sends one mail to alice.
    "./tests/e2e/dark-mails.e2e.mjs",
    // Stage 14, block D (decision 0041): a JMAP account through the setup form.
    "./tests/e2e/jmap.e2e.mjs",
    // Stage 15 (decision 0037): in-app updates against a local update server.
    "./tests/e2e/updates.e2e.mjs",
    // Stage 19 (decisions 0055 to 0057): source, redirect (to bob), edit as new; groups, vCard,
    // mbox into and out of Archive, the Thunderbird fixture. They append to INBOX and Archive.
    "./tests/e2e/stage19-message-tools.e2e.mjs",
    "./tests/e2e/stage19-contacts-folders.e2e.mjs",
    // Stage 21: sets up alice (writable) and bob (read-only) itself and removes them again.
    "./tests/e2e/context-menu.e2e.mjs",
    // Stage 22, items 3 and 8 (decisions 0072, 0073): rows without avatars, account dots, the rail,
    // contacts, the settings search; German, light and dark. Sets up alice and bob itself.
    "./tests/e2e/stage22-ui.e2e.mjs",
    // Stage 22, items 1, 2 and 12 (decisions 0078, 0081): account order and names, mail dragged
    // across accounts. Sets up alice and bob itself; the restart spec checks order and names after
    // the app started again and removes them.
    "./tests/e2e/stage22-accounts.e2e.mjs",
    "./tests/e2e/stage22-accounts-restart.e2e.mjs",
    // Stage 23, item 1 (decision 0082): accounts dragged in the column and the rail, files from
    // outside as bytes (a composer takes a PNG and a 5 MB file). Sets up and removes alice and bob.
    "./tests/e2e/stage23-drag.e2e.mjs",
    // Stage 21, item 4 (decision 0071): the spacing pass, every main view light and dark at
    // 1280×800 and 900×600, measured. Sets up alice and bob itself, appends `Spacing <run>`
    // messages, and leaves theme, density and the column as it found them.
    "./tests/e2e/spacing.e2e.mjs",
    // Stage 22, item 6 (decision 0077): the learning spam filter. Sets up alice itself, appends
    // offers to the junk folder and read letters to the inbox, moves one offer to Junk and back,
    // and removes the account and what the filter learned again.
    "./tests/e2e/spam-filter.e2e.mjs",
    "./tests/e2e/layouts.e2e.mjs",
  ],
  exclude: [...(README_SHOTS ? [] : ["./tests/e2e/readme-shots.e2e.mjs"]), ...(PERF ? [] : ["./tests/e2e/perf.e2e.mjs"])],
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
  mochaOpts: { ui: "bdd", timeout: 180000 },
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

    // A fresh test server with 10,000 messages for alice (9,959 in INBOX), or the 1,000 of the
    // screenshot rules with README_SHOTS=1; about 8 s. Only a SevereServiceError stops the run
    // here; any other error would be logged and the specs would start against no server.
    try {
      // Waits up to 30 minutes for another session's tests; prints "already held" if ours.
      ownLock = !testmailSync("lock", LOCK_NAME, "30").includes("already held");
      console.log(`test server lock: ${LOCK_NAME}${ownLock ? "" : " (held by the caller)"}`);
      // Stage 22 (decision 0075): under the lock, so no other run's app is using the keychain.
      const leftover = removeLeftoverVault();
      if (leftover.length > 0) console.log(`removed keychain items an earlier run left: ${leftover.join(", ")}`);
      const seed = README_SHOTS ? ["1000", "--no-smtp"] : [PERF ? PERF_SEED : "10000", "--filler-folder", "INBOX"];
      const ms = prepareTestServer({ seed });
      console.log(`test server ready with ${seed[0]} messages after ${ms} ms`);
    } catch (e) {
      // onComplete does not run after a failed onPrepare: give the lock back here.
      if (ownLock) testmailSync("unlock", LOCK_NAME);
      ownLock = false;
      throw new SevereServiceError(`the test server could not be prepared: ${String(e.stderr ?? e.message).trim()}`);
    }
    process.env[EXTRA_CA_ENV] = CA_FILE;
    // External images from the counter server on 127.0.0.1 (reading.e2e.mjs); e2e builds only.
    process.env[LOOPBACK_IMAGES_ENV] = "1";
    // Decision 0037: the update manifest from updates.e2e.mjs's local server (port 18996); the
    // other specs leave automatic checks off, so nothing connects there before it.
    process.env.MAIL_APP_E2E_UPDATE_ENDPOINT = "http://127.0.0.1:18996/latest.json";
  },
  onComplete: () => {
    try {
      testmailSync("stop");
    } finally {
      if (ownLock) testmailSync("unlock", LOCK_NAME);
    }
  },
};
