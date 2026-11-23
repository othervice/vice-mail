import { screen, waitFor, within } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarInviteInfo, MessageExtras, MessageListItem } from "../src/services/ipc";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { emitCore } from "./support/events";
import { makeCalendarEvent, makeInvite, NO_UNSUBSCRIBE } from "./support/extras";
import { renderApp } from "./support/render";

/** Read-only: showing and handing on an invitation writes nothing. */
const ADA = makeAccount({ writesEnabled: false });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

// The card shows times in the user's zone: these tests are a user in Berlin (Node applies TZ at once).
const zone = process.env["TZ"];
beforeAll(() => {
  process.env["TZ"] = "Europe/Berlin";
});
afterAll(() => {
  if (zone === undefined) delete process.env["TZ"];
  else process.env["TZ"] = zone;
});

const MAIL = makeMessage({ id: 1_700_000_200, subject: "Invitation: Planning" });

/** 30 Nov 2026, 15:00 UTC: 10:00 in New York. */
const START = Date.UTC(2026, 10, 30, 15, 0);

const MEETING = makeInvite([
  makeCalendarEvent({
    summary: "Planning",
    location: "Room 4 / https://meet.example.org/abc",
    description: "Agenda:\n1. Budget\n2. Dates",
    organizer: { name: "Grace Hopper", email: "grace@example.org", role: null, status: null },
    attendees: [
      { name: "Ada Lovelace", email: "ada@vicemail.test", role: "required", status: "needs_action" },
      { name: null, email: "lin@example.net", role: "optional", status: null },
    ],
    attendeeCount: 7,
    start: { kind: "dateTime", utc: START, local: "2026-11-30T10:00:00", zone: "America/New_York", floating: false },
    end: { kind: "dateTime", utc: START + 3_600_000, local: "2026-11-30T11:00:00", zone: "America/New_York", floating: false },
    recurrence: {
      frequency: "weekly",
      interval: 2,
      count: null,
      until: { kind: "date", date: "2027-03-01" },
      byDay: ["MO"],
      byMonthDay: [],
      byMonth: [],
      bySetPos: [],
      rule: "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO;UNTIL=20270301",
    },
  }),
]);

function setup(invites: CalendarInviteInfo[], extra: Partial<MessageExtras> = {}, failures = {}) {
  const items: MessageListItem[] = [MAIL];
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: { [MAIL.id]: makeView(MAIL) },
    readingPreferences: { markReadOnOpen: "never" },
    extras: { extras: { [MAIL.id]: { unsubscribe: NO_UNSUBSCRIBE, invites, ...extra } } },
    failures,
  });
  return { core, ...renderApp() };
}

async function open(user: ReturnType<typeof renderApp>["user"]) {
  const [option] = await screen.findAllByRole("option");
  if (!option) throw new Error("no row");
  await user.click(option);
  return screen.findByRole("region", { name: "Message" });
}

describe("calendar invitations", () => {
  it("shows title, time in the own zone, the organizer's time, repetition, place and people", async () => {
    const { user } = setup([MEETING]);
    const pane = await open(user);
    const card = await within(pane).findByRole("region", { name: "Planning" });

    expect(card.dataset["kind"]).toBe("request");
    expect(within(card).getByTestId("invite-kind").textContent).toBe("Invitation");
    expect(within(card).getByRole("heading", { level: 3, name: "Planning" })).toBeTruthy();
    // 15:00 UTC is 16:00 in Berlin; the file's own wall-clock time (New York) is added.
    expect(within(card).getByTestId("invite-when").textContent?.replace(/[\u2009\u202f]/g, " ")).toBe("Mon, Nov 30, 2026, 4:00 – 5:00 PM");
    expect(within(card).getByTestId("invite-original").textContent).toBe(
      "Mon, Nov 30, 2026, 10:00 AM in America/New_York, as the invitation states it",
    );
    expect(within(card).getByTestId("invite-recurrence").textContent).toBe("Every 2 weeks on Monday, until Mon, Mar 1, 2027");
    expect(within(card).getByTestId("invite-location").textContent).toBe("Room 4 / https://meet.example.org/abc");
    expect(within(card).getByTestId("invite-organizer").textContent).toBe("Grace Hopper <grace@example.org>");
    expect(within(card).getByTestId("invite-attendees").textContent).toBe("Ada Lovelace <ada@vicemail.test> and lin@example.net and 5 more");
    // The description is plain text: links in it are not clickable.
    const description = within(card).getByTestId("invite-description");
    expect(description.textContent).toBe("Agenda:\n1. Budget\n2. Dates");
    expect(within(card).queryAllByRole("link")).toEqual([]);
    // No answering from here (stage 9: no iTIP replies); the hint says where.
    expect(within(card).queryByRole("button", { name: /accept|decline/i })).toBeNull();
    expect(card.textContent).toContain("Answer in your calendar app");
  });

  it("an all-day event shows its day once (exclusive end), in German too", async () => {
    const allDay = makeInvite([
      makeCalendarEvent({ summary: "Betriebsausflug", start: { kind: "date", date: "2026-12-11" }, end: { kind: "date", date: "2026-12-12" } }),
    ]);
    const { user } = setup([allDay]);
    const pane = await open(user);
    const card = await within(pane).findByTestId("invite-card");
    expect(within(card).getByTestId("invite-when").textContent).toBe("Fri, Dec 11, 2026, all day");
    expect(within(card).queryByTestId("invite-original")).toBeNull();
  });

  it("a cancellation is marked as cancelled", async () => {
    const cancel = makeInvite([makeCalendarEvent({ summary: "Planning", cancelled: true, sequence: 3 })], { method: "CANCEL" });
    const { user } = setup([cancel]);
    const pane = await open(user);
    const card = await within(pane).findByTestId("invite-card");
    expect(card.dataset["kind"]).toBe("cancel");
    expect(within(card).getByTestId("invite-kind").textContent).toBe("Cancelled");
    expect(within(card).getByTestId("invite-no-time").textContent).toBe("No time given");
    expect(card.textContent).not.toContain("Answer in your calendar app");
  });

  it("an answer says who answered and how", async () => {
    const reply = makeInvite(
      [makeCalendarEvent({ attendees: [{ name: "Lin", email: "lin@example.net", role: null, status: "declined" }], attendeeCount: 1 })],
      { method: "REPLY" },
    );
    const { user } = setup([reply]);
    const pane = await open(user);
    const card = await within(pane).findByTestId("invite-card");
    expect(within(card).getByTestId("invite-kind").textContent).toBe("Answer to an invitation");
    expect(within(card).getByTestId("invite-answer").textContent).toBe("Lin <lin@example.net> declined.");
  });

  it("a damaged file shows what could be read and names the problems once each", async () => {
    const broken = makeInvite([makeCalendarEvent({ summary: null })], {
      method: null,
      problems: [
        { code: "truncated", detail: null },
        { code: "unknown_time_zone", detail: "Mars/Olympus" },
        { code: "unknown_time_zone", detail: "Mars/Olympus" },
      ],
    });
    const empty = makeInvite([], { method: null, problems: [{ code: "not_calendar", detail: null }] }, 3);
    const { user } = setup([broken, empty]);
    const pane = await open(user);
    const cards = await within(pane).findAllByTestId("invite-card");
    expect(cards).toHaveLength(2);
    const [first, second] = cards as [HTMLElement, HTMLElement];
    expect(within(first).getByRole("heading", { level: 3 }).textContent).toBe("Untitled event");
    const problems = within(first).getByTestId("invite-problems");
    expect(within(problems).getAllByRole("listitem").map((item) => item.textContent)).toEqual([
      "The file ends too early; what was read is shown.",
      "Unknown time zone (Mars/Olympus): the time is shown as written.",
    ]);
    expect(second.textContent).toContain("This calendar file has no event to show.");
    expect(second.textContent).toContain("The attachment is not a calendar file.");
  });

  it("“Open in calendar” hands the part to the system, and shows a failure with its hint", async () => {
    const { core, user } = setup([MEETING]);
    const pane = await open(user);
    const card = await within(pane).findByTestId("invite-card");
    await user.click(within(card).getByRole("button", { name: "Open in calendar" }));
    await within(card).findByTestId("invite-opened");
    expect(core.callsOf("open_invite")).toEqual([{ messageId: MAIL.id, partId: 2 }]);
    expect(core.extras.state.openedInvites).toEqual([{ messageId: MAIL.id, partId: 2 }]);
  });

  it("a failure to hand it on is shown where it happened", async () => {
    const { user } = setup([MEETING], {}, { open_invite: ipcError("internal", "no calendar app") });
    const pane = await open(user);
    const card = await within(pane).findByTestId("invite-card");
    await user.click(within(card).getByRole("button", { name: "Open in calendar" }));
    const error = await within(card).findByTestId("invite-open-error");
    expect(error.textContent).toContain("The invitation could not be handed to your calendar");
    expect(error.dataset["errorCode"]).toBe("internal");
  });

  it("appears once the body is stored: asks again after messageBodyAvailable", async () => {
    const { core, user } = setup([], { bodyComplete: false });
    const pane = await open(user);
    await waitFor(() => {
      expect(core.callsOf("get_message_extras")).toHaveLength(1);
    });
    expect(within(pane).queryByTestId("invite-card")).toBeNull();

    core.extras.setExtras(MAIL.id, { invites: [MEETING], bodyComplete: true });
    await emitCore({ type: "messageBodyAvailable", accountId: ADA.id, messageId: MAIL.id });
    expect(await within(pane).findByTestId("invite-card")).toBeTruthy();
    expect(core.callsOf("get_message_extras")).toHaveLength(2);
  });

  it("does not ask again for a message whose body was complete already", async () => {
    const { core, user } = setup([MEETING]);
    const pane = await open(user);
    await within(pane).findByTestId("invite-card");
    await emitCore({ type: "messageBodyAvailable", accountId: ADA.id, messageId: MAIL.id });
    await new Promise((resolve) => setTimeout(resolve, 40));
    expect(core.callsOf("get_message_extras")).toHaveLength(1);
  });
});
