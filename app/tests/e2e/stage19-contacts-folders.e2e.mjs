// Stage 19, part C (decisions 0056, 0057) in the real app against the local test server. The
// system's file dialogs are answered by the desktop-test hook `e2e_dialog_answers`.
//
// 1. Groups: a group created on the contacts page with two members; in the composer its name
//    suggests the group, which becomes its two members.
// 2. vCard round trip: "Export all contacts" writes a .vcf with the group; the group is deleted;
//    "Import vCard…" of that file brings it back with its members.
// 3. mbox round trip: the Thunderbird fixture's Inbox imported into Archive through the outbox
//    (two messages, the deleted one left out), then Archive exported as mbox containing them.
// 4. Thunderbird takeover from the fixture profile (copied; logins.json and key4.db unreadable):
//    the preview lists the IMAP and POP3 accounts and two local folders; alice's addresses and
//    signatures are taken over (alice is set up already); a local folder imports into Archive.
// 5. No CSP violation in the main frame.
//
// Screenshots: s19-groups.png, s19-composer-group.png, s19-takeover-preview.png; facts in
// s19-contacts-folders-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, invoke, openSettings, openSidebarParts, openView, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
import { ALICE, REPO, SERVER, serverFolders } from "./support/testmail.mjs";

const RUN = Date.now().toString(36);
const GROUP = `S19 team ${RUN}`;
const report = { screenshots: [], steps: {} };
const work = fs.mkdtempSync(path.join(os.tmpdir(), "s19-c-"));
let account;

const answer = (...paths) => coreCall("e2e_dialog_answers", { paths });

async function waitForJob(kind) {
  let job = null;
  await browser.waitUntil(
    async () => {
      const jobs = await coreCall("list_mbox_jobs");
      job = jobs.filter((j) => j.kind === kind).at(-1) ?? null;
      return job !== null && job.state !== "running";
    },
    { timeout: 120000, interval: 500, timeoutMsg: `the ${kind} job did not end` },
  );
  return job;
}

async function folderMenu(name) {
  const column = await testId("sidebar-column");
  if (!(await column.isExisting())) {
    await testId("rail-folders").click();
    await column.waitForExist({ timeout: 10000 });
  }
  // Stage 23 (decision 0085): folders other than the inbox and Sent are under "More folders".
  await openSidebarParts();
  const button = await $(`button[aria-label="Options for ${name}"]`);
  await button.waitForExist({ timeout: 20000 });
  await button.click();
  await $('[role="menu"]').waitForExist({ timeout: 5000 });
}

function copyFixture() {
  const copy = (from, to) => {
    fs.mkdirSync(to, { recursive: true });
    for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
      const source = path.join(from, entry.name);
      const target = path.join(to, entry.name);
      if (entry.isDirectory()) copy(source, target);
      else fs.copyFileSync(source, target);
    }
  };
  const base = path.join(work, "Thunderbird");
  const profile = path.join(base, "Profiles", "t3stpr0f.default-release");
  if (!fs.existsSync(base)) {
    copy(path.join(REPO, "testmail", "thunderbird"), base);
    for (const name of ["logins.json", "key4.db"]) fs.chmodSync(path.join(profile, name), 0o000);
  }
  return { base, profile };
}

describe("stage 19: contact groups, vCard, mbox, Thunderbird", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    account = await coreCall("add_account", {
      settings: {
        email: ALICE.email,
        displayName: "Alice Archer",
        imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: ALICE.email },
        smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: ALICE.email },
      },
      password: ALICE.password,
    });
    await browser.waitUntil(
      async () => {
        const [a] = await coreCall("list_accounts");
        return a?.status.state === "idle" && a.status.lastSyncAt !== null;
      },
      { timeout: 240000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    // The one-time "Check for updates automatically?" card would cover the screenshots.
    const no = await $("button=No, thanks");
    if (await no.isExisting()) await no.click();
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    for (const list of await coreCall("list_contact_lists").catch(() => [])) await coreCall("delete_contact_list", { id: list.id }).catch(() => undefined);
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s19-contacts-folders-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("creates a group and expands it in the composer", async () => {
    await openView("contacts");
    await testId("contacts-tab-groups").click();
    await testId("contact-group-new").click();
    await testId("contact-group-name-input").setValue(GROUP);
    await testId("contact-group-create").click();
    await testId("contact-group-detail").waitForExist({ timeout: 10000 });
    const field = await $('[data-testid="contact-group-add-field"] [data-testid="recipient-input"]');
    await field.setValue("bob@vicemail.test, grace@example.org,");
    await testId("contact-group-add-submit").click();
    await browser.waitUntil(async () => (await $$('[data-testid="contact-group-member"]').length) === 2, {
      timeoutMsg: "the members were not added",
    });
    report.screenshots.push(await screenshot("s19-groups"));

    await openView("inbox");
    await $('button[aria-label="New message"]').click();
    const to = await $('[data-testid="composer-to"] [data-testid="recipient-input"]');
    await to.waitForExist({ timeout: 20000 });
    await to.setValue("S19 team");
    const suggestion = await testId("recipient-group-suggestion");
    await suggestion.waitForExist({ timeout: 10000 });
    await suggestion.click();
    await browser.waitUntil(
      async () => (await $$('[data-testid="composer-to"] [data-testid="recipient-chip"]').length) === 2,
      { timeoutMsg: "the group did not become its members" },
    );
    report.screenshots.push(await screenshot("s19-composer-group"));
    report.steps.group = { members: 2 };
    await testId("composer-discard").click();
    await testId("composer-confirm-discard").waitForExist();
    await testId("composer-confirm-discard").$("button=Discard").click();
    await testId("composer").waitForExist({ reverse: true });
  });

  it("exports contacts as vCard and imports the file back", async () => {
    const file = path.join(work, "contacts.vcf");
    await openView("contacts");
    await answer(file);
    await $('button[aria-label="Export as vCard"]').click();
    await testId("contacts-export-all").click();
    await testId("contacts-export-saved").waitForExist({ timeout: 20000 });
    const text = fs.readFileSync(file, "utf8");
    assert.match(text, /BEGIN:VCARD/);
    assert.ok(text.includes(GROUP), "the group's card");
    const [list] = (await coreCall("list_contact_lists")).filter((l) => l.name === GROUP);
    await coreCall("delete_contact_list", { id: list.id });
    await answer(file);
    await testId("contacts-import").click();
    const reportBox = await testId("contacts-import-report");
    await reportBox.waitForExist({ timeout: 20000 });
    const back = (await coreCall("list_contact_lists")).find((l) => l.name === GROUP);
    assert.ok(back, "the group came back");
    assert.equal(back.memberCount, 2);
    report.steps.vcard = { bytes: text.length, report: await reportBox.getText() };
  });

  it("imports the Thunderbird Inbox into Archive and exports Archive as mbox", async () => {
    const { profile } = copyFixture();
    const source = path.join(profile, "Mail", "Local Folders", "Inbox");
    const before = (await serverFolders()).Archive?.messages ?? 0;
    await answer(source);
    await folderMenu("Archive");
    await $('[data-testid="folder-mbox-import"]').click();
    const imported = await waitForJob("import");
    assert.equal(imported.state, "done", JSON.stringify(imported));
    assert.equal(imported.done, 2);
    assert.equal(imported.skipped, 1, "the message Thunderbird deleted");
    await browser.waitUntil(async () => ((await serverFolders()).Archive?.messages ?? 0) === before + 2, {
      timeout: 30000,
      timeoutMsg: "the messages did not reach Archive",
    });

    const out = path.join(work, "archive.mbox");
    await answer(out);
    await folderMenu("Archive");
    await $('[data-testid="folder-mbox-export"]').click();
    const exported = await waitForJob("export");
    assert.equal(exported.state, "done", JSON.stringify(exported));
    const mbox = fs.readFileSync(out, "utf8");
    assert.match(mbox, /^From /);
    assert.ok(mbox.includes("TB fixture 1") && mbox.includes("TB fixture 2"), "the imported messages are in the export");
    assert.ok(!mbox.includes("TB fixture 3"), "the deleted one never was imported");
    report.steps.mbox = { imported: imported.done, skipped: imported.skipped, exported: exported.done, bytes: mbox.length };
  });

  it("takes over the fixture profile without reading its passwords", async () => {
    const { profile } = copyFixture();
    await openSettings("accounts");
    await testId("accounts-thunderbird").click();
    await testId("takeover").waitForExist({ timeout: 10000 });
    await answer(profile);
    await testId("takeover-choose").click();
    const preview = await testId("takeover-preview");
    await preview.waitForExist({ timeout: 20000 });
    assert.equal(await $$('[data-testid="takeover-account"]').length, 2);
    assert.equal(await $$('[data-testid="takeover-folder"]').length, 2);
    report.screenshots.push(await screenshot("s19-takeover-preview"));
    await testId("takeover-apply").click();
    await testId("takeover-applied").waitForExist({ timeout: 20000 });
    const identities = await coreCall("list_identities", { accountId: account.id });
    assert.ok(identities.some((i) => i.email === "alice.alias@vicemail.test"), JSON.stringify(identities));
    const signatures = await coreCall("list_signatures");
    assert.ok(signatures.length >= 2, JSON.stringify(signatures));
    report.steps.takeover = { identities: identities.map((i) => i.email), signatures: signatures.length };
    const result = await invoke("open_thunderbird_profile", { index: 99 });
    assert.ok("failure" in result, "no profile 99");
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
