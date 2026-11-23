import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { addableLabels } from "../src/features/reading/GmailLabels";
import { makeAccount, makeMailbox, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

/** Stage 14 (decision 0034): labels of a Gmail message in the reading pane. */

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

async function open(user: ReturnType<typeof renderApp>["user"]) {
  const [option] = await screen.findAllByRole("option");
  if (!option) throw new Error("no row");
  await user.click(option);
  return screen.findByRole("region", { name: "Message" });
}

describe("Gmail labels", () => {
  it("lists only ordinary folders as labels to add, without Gmail's own", () => {
    const boxes = [
      ...standardMailboxes(),
      makeMailbox({ id: 20, path: "[Gmail]/All Mail", role: "all" }),
      makeMailbox({ id: 21, path: "[Gmail]/Important" }),
      makeMailbox({ id: 22, path: "Work" }),
    ];
    expect(addableLabels(boxes, ["\\Inbox", "Projekte"])).toEqual(["Projekte/2026", "Work"]);
  });

  it("shows the labels and changes them through the core when writes are on", async () => {
    const account = makeAccount({ writesEnabled: true });
    const message = makeMessage({ subject: "Label me" });
    const boxes = [...standardMailboxes(), makeMailbox({ id: 22, path: "Work" })];
    const core = mockCore({
      accounts: [account],
      mailboxes: boxes,
      messages: { 1: [message] },
      views: { [message.id]: makeView(message) },
      gmailLabels: { [message.id]: ["\\Inbox", "\\Important", "Projekte"] },
    });
    const { user } = renderApp();
    const pane = await open(user);
    const labels = await within(pane).findByRole("list", { name: "Labels" });
    expect(within(labels).getByText("Important")).toBeTruthy();
    expect(within(labels).getByText("Projekte")).toBeTruthy();
    expect(within(labels).queryByText("\\Inbox")).toBeNull();

    await user.click(within(labels).getByRole("button", { name: "Remove label “Projekte”" }));
    await waitFor(() => {
      expect(core.callsOf("set_message_label")).toHaveLength(1);
    });
    expect(core.callsOf("set_message_label")[0]).toMatchObject({ messageId: message.id, label: "Projekte", add: false });
    // The chip goes; the folder is offered to add again.
    await waitFor(() => {
      expect(within(labels).queryByRole("button", { name: "Remove label “Projekte”" })).toBeNull();
    });

    await user.selectOptions(within(labels).getByTestId("gmail-label-add"), "Work");
    await waitFor(() => {
      expect(core.callsOf("set_message_label")).toHaveLength(2);
    });
    expect(core.callsOf("set_message_label")[1]).toMatchObject({ label: "Work", add: true });
  });

  it("shows nothing for other servers and offers no changes on read-only accounts", async () => {
    const message = makeMessage({ subject: "Plain IMAP" });
    mockCore({
      accounts: [makeAccount()],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message) },
    });
    const { user } = renderApp();
    const pane = await open(user);
    await within(pane).findByTestId("message-card");
    expect(within(pane).queryByTestId("gmail-labels")).toBeNull();
  });
});
