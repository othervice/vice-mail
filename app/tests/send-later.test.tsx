import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fromLocalInput,
  SendLaterPicker,
  sendLaterPresets,
  toLocalInput,
  validateSendAt,
} from "../src/features/composer/SendLaterPicker";
import { createI18n } from "../src/i18n";
import { CoreError, MAX_SCHEDULE_AHEAD_MS, type OutgoingMail } from "../src/services/ipc";
import { useComposerStore } from "../src/stores/composer";
import { useUiStore } from "../src/stores/ui";
import { makeDraft, makeOutgoing, type ComposeFakeOptions } from "./support/compose";
import { ipcError, makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ADA = makeAccount({ writesEnabled: true });
const HOUR = 3_600_000;

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

function setup(compose: ComposeFakeOptions = {}, language: "en" | "de" = "en") {
  const items = makeMessages(2);
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    compose,
  });
  return { core, ...renderApp(language) };
}

/** A frozen draft and the send the user planned for it. */
function planned(overrides: Partial<OutgoingMail> = {}) {
  const draft = makeDraft({ id: "d-plan", subject: "Angebot", to: [{ name: null, email: "grace@example.org" }], state: "sending", outgoing: 70 });
  const mail = makeOutgoing({
    operationId: 70,
    draftId: draft.id,
    subject: "Angebot",
    state: "scheduled",
    scheduled: true,
    sendAt: Date.now() + 5 * HOUR,
    ...overrides,
  });
  return { draft, mail };
}

/** A send that was due while the app was off, more than an hour ago. */
function missed(overrides: Partial<OutgoingMail> = {}) {
  const draft = makeDraft({ id: "d-missed", subject: "Bericht", to: [{ name: null, email: "lin@example.net" }], state: "sending", outgoing: 71 });
  const mail = makeOutgoing({
    operationId: 71,
    draftId: draft.id,
    subject: "Bericht",
    state: "missed",
    scheduled: true,
    sendAt: Date.now() - 3 * HOUR,
    error: ipcError("send_missed"),
    ...overrides,
  });
  return { draft, mail };
}

async function showOutbox() {
  await screen.findByRole("button", { name: "New message" });
  act(() => {
    useUiStore.getState().showOutbox();
  });
  return screen.findByTestId("outbox-list");
}

describe("send later: suggested times and checks", () => {
  it("suggests in one hour, tomorrow 08:00 and next Monday 08:00 in local time", () => {
    // Monday, 23 Nov 2026, 07:30:20 local time.
    const now = new Date(2026, 10, 23, 7, 30, 20);
    const presets = sendLaterPresets(now);
    expect(presets.map((p) => p.id)).toEqual(["inOneHour", "tomorrowMorning", "nextMonday"]);
    expect(new Date(presets[0]?.at ?? 0)).toEqual(new Date(2026, 10, 23, 8, 31, 0));
    expect(new Date(presets[1]?.at ?? 0)).toEqual(new Date(2026, 10, 24, 8, 0, 0));
    // On a Monday, "next Monday" is a week ahead.
    expect(new Date(presets[2]?.at ?? 0)).toEqual(new Date(2026, 10, 30, 8, 0, 0));
  });

  it("leaves out Monday when that is tomorrow", () => {
    // Sunday, 29 Nov 2026.
    const presets = sendLaterPresets(new Date(2026, 10, 29, 20, 0, 0));
    expect(presets.map((p) => p.id)).toEqual(["inOneHour", "tomorrowMorning"]);
  });

  it("refuses times in the past and more than a year ahead, like the core", () => {
    const now = Date.UTC(2026, 10, 23, 7, 0);
    expect(validateSendAt(now, now)).toBe("past");
    expect(validateSendAt(now - 1, now)).toBe("past");
    expect(validateSendAt(Number.NaN, now)).toBe("past");
    expect(validateSendAt(now + 60_000, now)).toBeNull();
    expect(validateSendAt(now + MAX_SCHEDULE_AHEAD_MS, now)).toBeNull();
    expect(validateSendAt(now + MAX_SCHEDULE_AHEAD_MS + 1, now)).toBe("tooFar");
  });

  it("reads and writes the date field in local time", () => {
    const at = new Date(2026, 11, 24, 18, 5).getTime();
    expect(toLocalInput(at)).toBe("2026-12-24T18:05");
    expect(fromLocalInput("2026-12-24T18:05")).toBe(at);
    expect(Number.isNaN(fromLocalInput(""))).toBe(true);
  });
});

describe("send later: the picker", () => {
  function renderPicker(props: Partial<Parameters<typeof SendLaterPicker>[0]> = {}, language: "en" | "de" = "en") {
    const onChoose = vi.fn();
    const onCancel = vi.fn();
    const user = userEvent.setup();
    render(
      <I18nextProvider i18n={createI18n(language)}>
        <SendLaterPicker confirmLabel="Schedule" onChoose={onChoose} onCancel={onCancel} {...props} />
      </I18nextProvider>,
    );
    return { onChoose, onCancel, user, picker: screen.getByTestId("send-later-picker") };
  }

  it("says plainly that it sends only while the app runs and is online, and focuses the first choice", () => {
    const { picker } = renderPicker();
    expect(picker.getAttribute("role")).toBe("group");
    expect(screen.getByRole("group", { name: "Send later" })).toBe(picker);
    expect(picker.textContent).toContain("Sent at this time only if the app is running and online then.");
    expect(document.activeElement).toBe(screen.getByTestId("send-later-preset-inOneHour"));
  });

  it("says it in German too", () => {
    renderPicker({}, "de");
    expect(screen.getByTestId("send-later-hint").textContent).toBe(
      "Wird nur gesendet, wenn die App zu diesem Zeitpunkt läuft und online ist.",
    );
  });

  it("chooses a suggestion at once", async () => {
    const { onChoose, user } = renderPicker();
    const before = Date.now();
    await user.click(screen.getByTestId("send-later-preset-tomorrowMorning"));
    const sendAt = onChoose.mock.calls[0]?.[0] as number;
    expect(sendAt).toBeGreaterThan(before);
    expect(new Date(sendAt).getHours()).toBe(8);
    expect(new Date(sendAt).getMinutes()).toBe(0);
  });

  it("takes a date and time of the user's choice and refuses one in the past", async () => {
    const { onChoose, user } = renderPicker();
    const field = screen.getByLabelText("Date and time");
    fireEvent.change(field, { target: { value: "2020-01-01T09:00" } });
    await user.click(screen.getByRole("button", { name: "Schedule" }));
    expect(onChoose).not.toHaveBeenCalled();
    expect(screen.getByTestId("send-later-problem").textContent).toContain("already passed");
    expect(field.getAttribute("aria-invalid")).toBe("true");

    const later = Date.now() + 2 * 24 * HOUR;
    fireEvent.change(field, { target: { value: toLocalInput(later) } });
    expect(screen.queryByTestId("send-later-problem")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Schedule" }));
    expect(onChoose).toHaveBeenCalledWith(fromLocalInput(toLocalInput(later)));
  });

  it("starts at the time of a send being moved", () => {
    const at = new Date(2027, 0, 5, 9, 30).getTime();
    renderPicker({ initial: at });
    expect(screen.getByLabelText<HTMLInputElement>("Date and time").value).toBe("2027-01-05T09:30");
  });

  it("Escape cancels without reaching the surrounding panel", async () => {
    const outer = vi.fn();
    const onCancel = vi.fn();
    const user = userEvent.setup();
    render(
      <I18nextProvider i18n={createI18n("en")}>
        <div onKeyDown={outer}>
          <SendLaterPicker confirmLabel="Schedule" onChoose={vi.fn()} onCancel={onCancel} />
        </div>
      </I18nextProvider>,
    );
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });

  it("shows a typed error from the core", () => {
    renderPicker({ error: new CoreError(ipcError("message_too_large")) });
    const error = screen.getByTestId("send-later-error");
    expect(error.getAttribute("data-error-code")).toBe("message_too_large");
    expect(error.textContent).toContain("The message is larger than your mail server accepts.");
  });
});

describe("send later: the outbox", () => {
  it("shows a planned send with its time and the rule, and moves it", async () => {
    const { draft, mail } = planned();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const list = await showOutbox();
    expect(screen.getByTestId("outbox-send-later-rule").textContent).toContain("only if the app is running and online");
    const row = within(list).getByTestId("outgoing-row");
    expect(within(row).getByTestId("outgoing-state").textContent).toBe("Planned");
    expect(within(row).getByTestId("outgoing-planned").textContent).toContain("Planned for");
    // No undo countdown for a time the user chose.
    expect(within(row).queryByRole("button", { name: "Undo" })).toBeNull();

    await user.click(within(row).getByRole("button", { name: "Change time…" }));
    const picker = within(row).getByTestId("outgoing-picker");
    const at = Date.now() + 26 * HOUR;
    fireEvent.change(within(picker).getByLabelText("Date and time"), { target: { value: toLocalInput(at) } });
    await user.click(within(picker).getByRole("button", { name: "Schedule" }));
    await waitFor(() => {
      expect(core.callsOf("reschedule_send")).toEqual([{ operationId: 70, when: { sendAt: fromLocalInput(toLocalInput(at)) } }]);
    });
    await waitFor(() => {
      expect(within(screen.getByTestId("outbox-list")).queryByTestId("outgoing-picker")).toBeNull();
    });
  });

  it("a refused new time stays in the picker with the reason", async () => {
    const { draft, mail } = planned();
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [] },
      compose: { drafts: [draft], outgoing: [mail] },
      failures: { reschedule_send: ipcError("send_in_progress") },
    });
    const { user } = renderApp();
    const list = await showOutbox();
    const row = within(list).getByTestId("outgoing-row");
    await user.click(within(row).getByRole("button", { name: "Change time…" }));
    await user.click(within(row).getByTestId("send-later-preset-tomorrowMorning"));
    const error = await within(row).findByTestId("send-later-error");
    expect(error.getAttribute("data-error-code")).toBe("send_in_progress");
  });

  it("sends a planned send now, after the usual undo delay", async () => {
    const { draft, mail } = planned();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const list = await showOutbox();
    await user.click(within(within(list).getByTestId("outgoing-row")).getByRole("button", { name: "Send now" }));
    await waitFor(() => {
      expect(core.callsOf("reschedule_send")).toEqual([{ operationId: 70, when: null }]);
    });
    await waitFor(() => {
      expect(within(screen.getByTestId("outbox-list")).getByTestId("outgoing-row").getAttribute("data-send-state")).toBe("scheduled");
    });
    expect(core.compose.outgoing[0]?.scheduled).toBe(false);
  });

  it("cancelling takes the message back into the composer as a draft", async () => {
    const { draft, mail } = planned();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const list = await showOutbox();
    await user.click(within(within(list).getByTestId("outgoing-row")).getByRole("button", { name: "Cancel sending" }));
    await waitFor(() => {
      expect(core.callsOf("cancel_send")).toEqual([{ draftId: "d-plan" }]);
    });
    expect((await screen.findByLabelText<HTMLInputElement>("Subject")).value).toBe("Angebot");
    expect(core.compose.drafts.get("d-plan")?.state).toBe("editing");
  });

  it("asks about a missed send in its row: send now, schedule again, keep as draft", async () => {
    const { draft, mail } = missed();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const list = await showOutbox();
    const row = within(list).getByTestId("outgoing-row");
    expect(within(row).getByTestId("outgoing-missed").textContent).toContain("was not sent");
    await user.click(within(row).getByRole("button", { name: "Schedule again…" }));
    await user.click(within(row).getByTestId("send-later-preset-inOneHour"));
    await waitFor(() => {
      expect(core.callsOf("resolve_missed_send")).toHaveLength(1);
    });
    const decision = core.callsOf("resolve_missed_send")[0]?.["decision"] as { kind: string; sendAt: number };
    expect(decision.kind).toBe("reschedule");
    expect(decision.sendAt).toBeGreaterThan(Date.now());
    await waitFor(() => {
      expect(within(screen.getByTestId("outbox-list")).getByTestId("outgoing-state").textContent).toBe("Planned");
    });
  });
});

describe("send later: a send missed while the app was off", () => {
  it("asks at the start for every missed send, even before any event", async () => {
    const { draft, mail } = missed();
    const { core } = setup({ drafts: [draft], outgoing: [mail] });
    const notice = await screen.findByTestId("send-notice");
    expect(core.callsOf("list_scheduled_sends")).toHaveLength(1);
    expect(notice.getAttribute("role")).toBe("alert");
    expect(notice.getAttribute("data-send-state")).toBe("missed");
    expect(notice.textContent).toContain("was not sent");
    expect(notice.textContent).toContain("The app was not running at that time.");
    // Nothing is sent unless the user answers.
    expect(core.callsOf("resolve_missed_send")).toHaveLength(0);
    expect(core.callsOf("retry_operation")).toHaveLength(0);
    expect(within(notice).getByRole("button", { name: "Send now" })).toBeTruthy();
    expect(within(notice).getByRole("button", { name: "Schedule again…" })).toBeTruthy();
    expect(within(notice).getByRole("button", { name: "Keep as draft" })).toBeTruthy();
  });

  it("“Send now” sends it after the undo delay", async () => {
    const { draft, mail } = missed();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const notice = await screen.findByTestId("send-notice");
    await user.click(within(notice).getByRole("button", { name: "Send now" }));
    await waitFor(() => {
      expect(core.callsOf("resolve_missed_send")).toEqual([{ operationId: 71, decision: { kind: "send_now" } }]);
    });
    await waitFor(() => {
      expect(screen.getByTestId("send-notice").getAttribute("data-send-state")).toBe("scheduled");
    });
    expect(screen.getByTestId("send-notice").textContent).toContain("Sending in");
  });

  it("“Schedule again” picks a new time in the notice", async () => {
    const { draft, mail } = missed();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const notice = await screen.findByTestId("send-notice");
    await user.click(within(notice).getByRole("button", { name: "Schedule again…" }));
    const picker = within(notice).getByTestId("missed-picker");
    expect(picker.textContent).toContain("only if the app is running and online");
    await user.click(within(picker).getByTestId("send-later-preset-tomorrowMorning"));
    await waitFor(() => {
      expect(core.callsOf("resolve_missed_send")[0]?.["decision"]).toMatchObject({ kind: "reschedule" });
    });
    await waitFor(() => {
      expect(screen.getByTestId("send-notice").textContent).toContain("Scheduled for");
    });
  });

  it("“Keep as draft” takes it out of the outbox and opens the draft", async () => {
    const { draft, mail } = missed();
    const { core, user } = setup({ drafts: [draft], outgoing: [mail] });
    const notice = await screen.findByTestId("send-notice");
    await user.click(within(notice).getByRole("button", { name: "Keep as draft" }));
    await waitFor(() => {
      expect(core.callsOf("resolve_missed_send")).toEqual([{ operationId: 71, decision: { kind: "keep_as_draft" } }]);
    });
    expect((await screen.findByLabelText<HTMLInputElement>("Subject")).value).toBe("Bericht");
    expect(screen.queryByTestId("send-notice")).toBeNull();
    expect(core.compose.outgoing).toHaveLength(0);
  });

  it("an answer the core refuses is reported", async () => {
    const { draft, mail } = missed();
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [] },
      compose: { drafts: [draft], outgoing: [mail] },
      failures: { resolve_missed_send: ipcError("send_in_progress") },
    });
    const { user } = renderApp();
    const notice = await screen.findByTestId("send-notice");
    await user.click(within(notice).getByRole("button", { name: "Send now" }));
    const problem = await screen.findByTestId("composer-problem");
    expect(problem.getAttribute("data-error-code")).toBe("send_in_progress");
    expect(problem.textContent).toContain("Your answer could not be recorded.");
  });

  it("the event during the session raises the same question", async () => {
    const { draft, mail } = missed();
    const { core } = setup({ drafts: [draft] });
    await screen.findByRole("button", { name: "New message" });
    await waitFor(() => {
      expect(core.callsOf("list_scheduled_sends")).toHaveLength(1);
    });
    expect(screen.queryByTestId("send-notice")).toBeNull();
    await emitCore({ type: "sendNeedsDecision", accountId: ADA.id, mail });
    const notice = await screen.findByTestId("send-notice");
    expect(notice.getAttribute("data-send-state")).toBe("missed");
    expect(within(notice).getByRole("button", { name: "Keep as draft" })).toBeTruthy();
  });

  it("a missed send from another window is shown by `outgoingChanged` too", async () => {
    const { mail } = missed();
    setup();
    await screen.findByRole("button", { name: "New message" });
    await emitCore({ type: "outgoingChanged", accountId: ADA.id, mails: [mail] });
    expect((await screen.findByTestId("send-notice")).getAttribute("data-send-state")).toBe("missed");
  });

  it("says so when the planned sends cannot be checked at the start", async () => {
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [] },
      failures: { list_scheduled_sends: ipcError("database") },
    });
    renderApp();
    const error = await screen.findByTestId("missed-list-error");
    expect(error.textContent).toContain("Messages planned for later could not be checked.");
  });

  it("a planned send shows its time and the rule instead of an undo countdown", async () => {
    const { mail } = planned();
    setup();
    await screen.findByRole("button", { name: "New message" });
    act(() => {
      useComposerStore.getState().track(mail);
    });
    const notice = await screen.findByTestId("send-notice");
    expect(notice.textContent).toContain("Scheduled for");
    expect(notice.textContent).toContain("only if the app is running and online");
    expect(within(notice).queryByRole("button", { name: "Undo" })).toBeNull();
    expect(within(notice).getByRole("button", { name: "Show outbox" })).toBeTruthy();
  });
});

describe("send later: the fake core keeps the contract", () => {
  it("schedules once per request id and refuses past times", async () => {
    const draft = makeDraft({ id: "d-f", to: [{ name: null, email: "grace@example.org" }] });
    const { core } = setup({ drafts: [draft] });
    await screen.findByRole("button", { name: "New message" });
    const { composerIpc } = await import("../src/services/ipc");
    await expect(composerIpc.scheduleSend("d-f", "r1", { sendAt: Date.now() - 1 })).rejects.toMatchObject({ code: "invalid_argument" });
    const first = await composerIpc.scheduleSend("d-f", "r1", { sendAt: Date.now() + HOUR });
    const again = await composerIpc.scheduleSend("d-f", "r1", { sendAt: Date.now() + HOUR });
    expect(again.operationId).toBe(first.operationId);
    expect(first).toMatchObject({ state: "scheduled", scheduled: true });
    expect(core.compose.drafts.get("d-f")?.state).toBe("sending");
    expect((await composerIpc.listScheduledSends()).map((m) => m.operationId)).toEqual([first.operationId]);
  });
});
