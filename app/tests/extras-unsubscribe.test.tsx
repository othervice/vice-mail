import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftUpdate, MessageListItem, UnsubscribeOptions } from "../src/services/ipc";
import { ipcError, makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { makeComposeRequest, NO_UNSUBSCRIBE, type ExtrasFakeOptions } from "./support/extras";
import { renderApp } from "./support/render";

const ADA = makeAccount();

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
});

const NEWSLETTER = makeMessage({ id: 1_700_000_100, subject: "Weekly digest", from: { name: "Digest", email: "news@lists.example.org" } });

const ALL_WAYS: UnsubscribeOptions = {
  oneClickHost: "lists.example.org",
  mailto: "leave@lists.example.org",
  web: "https://lists.example.org/unsubscribe?u=42",
};

function setup(unsubscribe: UnsubscribeOptions, extras: ExtrasFakeOptions = {}, items: MessageListItem[] = [NEWSLETTER], failures = {}) {
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: items },
    views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    readingPreferences: { markReadOnOpen: "never" },
    extras: { extras: { [NEWSLETTER.id]: { unsubscribe } }, ...extras },
    failures,
  });
  return { core, ...renderApp() };
}

async function open(user: ReturnType<typeof renderApp>["user"], index = 0) {
  const options = await screen.findAllByRole("option");
  const option = options[index];
  if (!option) throw new Error("no row");
  await user.click(option);
  return screen.findByRole("region", { name: "Message" });
}

describe("unsubscribing", () => {
  it("shows no button for a message without List-Unsubscribe", async () => {
    const { core, user } = setup(NO_UNSUBSCRIBE);
    const pane = await open(user);
    await waitFor(() => {
      expect(core.callsOf("get_message_extras")).toEqual([{ messageId: NEWSLETTER.id }]);
    });
    expect(within(pane).queryByTestId("unsubscribe-bar")).toBeNull();
    expect(within(pane).queryByTestId("message-extras")).toBeNull();
  });

  it("sends nothing before the click: not on opening, showing or unfolding the ways", async () => {
    const { core, user } = setup(ALL_WAYS);
    const pane = await open(user);
    const bar = await within(pane).findByTestId("unsubscribe-bar");
    expect(bar.textContent).toContain("This message comes from a mailing list.");
    expect(core.extras.state.unsubscribes).toEqual([]);

    await user.click(within(bar).getByRole("button", { name: "Unsubscribe" }));
    const ask = within(pane).getByRole("group", { name: "Unsubscribe from this list" });
    // The question takes the focus, so keyboard and screen reader users land on it.
    expect(document.activeElement).toBe(ask);
    expect(within(ask).getByTestId("unsubscribe-way-oneClick").textContent).toContain("lists.example.org");
    expect(within(ask).getByTestId("unsubscribe-way-mailto").textContent).toContain("leave@lists.example.org");
    expect(within(ask).getByTestId("unsubscribe-web-url").textContent).toBe("https://lists.example.org/unsubscribe?u=42");
    // One click is suggested first.
    expect(within(ask).getAllByRole("listitem").map((item) => item.dataset["testid"])).toEqual([
      "unsubscribe-way-oneClick",
      "unsubscribe-way-mailto",
      "unsubscribe-way-web",
    ]);
    expect(core.extras.state.unsubscribes).toEqual([]);
    expect(core.callsOf("unsubscribe")).toEqual([]);
  });

  it("one click: exactly one request after the click, then the result", async () => {
    const { core, user } = setup(ALL_WAYS);
    const pane = await open(user);
    await user.click(await within(pane).findByRole("button", { name: "Unsubscribe" }));
    await user.click(within(pane).getByRole("button", { name: "Unsubscribe now" }));

    const result = await within(pane).findByTestId("unsubscribe-result");
    expect(core.callsOf("unsubscribe")).toEqual([{ messageId: NEWSLETTER.id, method: "oneClick" }]);
    expect(within(result).getByRole("status").textContent).toContain("Unsubscribe request sent");
    expect(result.textContent).toContain("lists.example.org accepted it (HTTP 200)");
    expect(within(pane).queryByRole("button", { name: "Unsubscribe now" })).toBeNull();
  });

  it("by mail: opens a prepared draft from the message's account and sends nothing", async () => {
    const request = makeComposeRequest({ to: [{ name: null, email: "leave@lists.example.org" }], subject: "unsubscribe", body: "Please remove me." });
    const { core, user } = setup({ ...NO_UNSUBSCRIBE, mailto: "leave@lists.example.org" }, {
      unsubscribeOutcomes: { [`${String(NEWSLETTER.id)}/mailto`]: { kind: "compose", request } },
    });
    const pane = await open(user);
    await user.click(await within(pane).findByRole("button", { name: "Unsubscribe" }));
    await user.click(within(pane).getByRole("button", { name: "Write unsubscribe email" }));

    // The form replaces the composer's loading frame (another element).
    await screen.findByLabelText("Subject");
    const composer = screen.getByTestId("composer");
    expect(core.callsOf("create_draft")[0]?.["request"]).toEqual({
      kind: "new",
      accountId: ADA.id,
      identityId: null,
      to: [{ name: null, email: "leave@lists.example.org" }],
    });
    await waitFor(() => {
      expect(within(composer).getByLabelText<HTMLInputElement>("Subject").value).toBe("unsubscribe");
    });
    const update = core.callsOf("save_draft")[0]?.["update"] as DraftUpdate;
    expect(update.subject).toBe("unsubscribe");
    expect(update.bodyText).toBe("Please remove me.");
    expect(core.callsOf("send_draft")).toEqual([]);
    expect(within(pane).getByTestId("unsubscribe-result").dataset["outcome"]).toBe("compose");
  });

  it("on the web: the full address is shown first, the browser opens only after the click", async () => {
    const { core, user } = setup({ ...NO_UNSUBSCRIBE, web: "https://lists.example.org/u/1" });
    const pane = await open(user);
    await user.click(await within(pane).findByRole("button", { name: "Unsubscribe" }));
    expect(within(pane).getByTestId("unsubscribe-web-url").textContent).toBe("https://lists.example.org/u/1");
    expect(within(pane).queryByRole("button", { name: "Unsubscribe now" })).toBeNull();
    expect(core.callsOf("unsubscribe")).toEqual([]);

    await user.click(within(pane).getByRole("button", { name: "Open in browser" }));
    expect((await within(pane).findByTestId("unsubscribe-result")).textContent).toContain("Page opened in your browser");
    expect(core.callsOf("unsubscribe")).toEqual([{ messageId: NEWSLETTER.id, method: "web" }]);
  });

  it("shows a failure with its title and hint, and tries again only on a click", async () => {
    const { core, user } = setup(ALL_WAYS, {}, [NEWSLETTER], { unsubscribe: ipcError("server_rejected", "HTTP 500") });
    const pane = await open(user);
    await user.click(await within(pane).findByRole("button", { name: "Unsubscribe" }));
    await user.click(within(pane).getByRole("button", { name: "Unsubscribe now" }));

    const error = await within(pane).findByTestId("unsubscribe-error");
    expect(error.dataset["errorCode"]).toBe("server_rejected");
    expect(error.textContent).toContain("Unsubscribing did not work");
    expect(core.callsOf("unsubscribe")).toHaveLength(1);
    await user.click(within(error).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("unsubscribe")).toHaveLength(2);
    });
    expect(core.callsOf("unsubscribe")[1]).toEqual({ messageId: NEWSLETTER.id, method: "oneClick" });
  });

  it("Escape or Cancel closes the question without a request, the focus goes back to the button", async () => {
    const { core, user } = setup(ALL_WAYS);
    const pane = await open(user);
    await user.click(await within(pane).findByRole("button", { name: "Unsubscribe" }));
    fireEvent.keyDown(within(pane).getByTestId("unsubscribe-ask"), { key: "Escape" });
    await waitFor(() => {
      expect(document.activeElement).toBe(within(pane).getByRole("button", { name: "Unsubscribe" }));
    });
    // Escape closed the question, not the message.
    expect(screen.getByRole("region", { name: "Message" })).toBeTruthy();

    await user.keyboard("{Enter}");
    await user.click(within(pane).getByRole("button", { name: "Cancel" }));
    expect(within(pane).queryByTestId("unsubscribe-ask")).toBeNull();
    expect(core.callsOf("unsubscribe")).toEqual([]);
  });

  it("is keyboard operable: Tab to the way, Enter sends", async () => {
    const { core, user } = setup({ ...NO_UNSUBSCRIBE, oneClickHost: "lists.example.org" });
    const pane = await open(user);
    (await within(pane).findByRole("button", { name: "Unsubscribe" })).focus();
    await user.keyboard("{Enter}");
    await user.tab();
    expect(document.activeElement).toBe(within(pane).getByRole("button", { name: "Unsubscribe now" }));
    await user.keyboard("{Enter}");
    await within(pane).findByTestId("unsubscribe-result");
    expect(core.callsOf("unsubscribe")).toEqual([{ messageId: NEWSLETTER.id, method: "oneClick" }]);
  });

  it("a load failure of the options is shown with a way to try again", async () => {
    const { user } = setup(ALL_WAYS, {}, [NEWSLETTER], { get_message_extras: ipcError("database") });
    const pane = await open(user);
    const error = await within(pane).findByTestId("extras-error");
    expect(error.dataset["errorCode"]).toBe("database");
    expect(within(error).getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  it("the next message starts without the answer of the last one", async () => {
    const second = makeMessage({ id: 1_700_000_050, subject: "Another digest" });
    const { user } = setup(ALL_WAYS, { extras: { [NEWSLETTER.id]: { unsubscribe: ALL_WAYS }, [second.id]: { unsubscribe: ALL_WAYS } } }, [NEWSLETTER, second]);
    let pane = await open(user, 0);
    await user.click(await within(pane).findByRole("button", { name: "Unsubscribe" }));
    await user.click(within(pane).getByRole("button", { name: "Unsubscribe now" }));
    await within(pane).findByTestId("unsubscribe-result");

    pane = await open(user, 1);
    await waitFor(() => {
      expect(within(pane).getByRole("heading", { level: 2, name: "Another digest" })).toBeTruthy();
    });
    expect(await within(pane).findByTestId("unsubscribe-bar")).toBeTruthy();
    expect(within(pane).queryByTestId("unsubscribe-result")).toBeNull();
  });
});
