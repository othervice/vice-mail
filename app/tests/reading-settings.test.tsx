import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ipcError, makeAccount, mockCore, PREFETCH_DEFAULTS } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const MIB = 1024 * 1024;
const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "ada@work.test", displayName: "", color: 3 });

async function openReadingSettings(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Reading" }));
  return screen.getByRole("tabpanel");
}

describe("Settings, Reading", () => {
  it("shows the prefetch period and size limit and stores a change", async () => {
    const core = mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const panel = await openReadingSettings(user);

    const period = await within(panel).findByRole("radiogroup", { name: "Messages from the last" });
    expect(within(period).getAllByRole("radio").map((r) => r.textContent)).toEqual([
      "7 days",
      "30 days",
      "90 days",
      "Year",
      "Everything",
    ]);
    expect(within(period).getByRole("radio", { name: "30 days" }).getAttribute("aria-checked")).toBe("true");
    const size = within(panel).getByRole("radiogroup", { name: "Whole messages up to" });
    expect(within(size).getAllByRole("radio").map((r) => r.textContent)).toEqual(["Text only", "1 MB", "5 MB", "25 MB", "100 MB"]);
    expect(within(size).getByRole("radio", { name: "5 MB" }).getAttribute("aria-checked")).toBe("true");

    await user.click(within(period).getByRole("radio", { name: "90 days" }));
    await user.click(within(size).getByRole("radio", { name: "25 MB" }));

    expect(core.callsOf("set_prefetch_settings")).toEqual([
      { settings: { period: "days90", maxMessageSize: PREFETCH_DEFAULTS.maxMessageSize } },
      { settings: { period: "days90", maxMessageSize: 25 * MIB } },
    ]);
    expect(core.prefetchSettings).toEqual({ period: "days90", maxMessageSize: 25 * MIB });
    expect(within(size).getByRole("radio", { name: "25 MB" }).getAttribute("aria-checked")).toBe("true");
  });

  it("keeps a size limit it does not offer selectable as it is", async () => {
    mockCore({ accounts: [ADA], prefetchSettings: { period: "all", maxMessageSize: 10 * MIB } });
    const { user } = renderApp();
    const panel = await openReadingSettings(user);

    const size = await within(panel).findByRole("radiogroup", { name: "Whole messages up to" });
    expect(within(size).getByRole("radio", { name: "10 MB" }).getAttribute("aria-checked")).toBe("true");
    expect(within(panel).getByRole("radio", { name: "Everything" }).getAttribute("aria-checked")).toBe("true");
  });

  it("rolls a refused change back and says so", async () => {
    mockCore({ accounts: [ADA], failures: { set_prefetch_settings: ipcError("invalid_argument") } });
    const { user } = renderApp();
    const panel = await openReadingSettings(user);
    const period = await within(panel).findByRole("radiogroup", { name: "Messages from the last" });

    await user.click(within(period).getByRole("radio", { name: "7 days" }));

    expect((await within(panel).findByRole("alert")).textContent).toContain("Your change could not be saved.");
    await waitFor(() => {
      expect(within(period).getByRole("radio", { name: "30 days" }).getAttribute("aria-checked")).toBe("true");
    });
  });

  it("shows the prefetch state of each account and follows prefetchProgress", async () => {
    mockCore({
      accounts: [ADA, WORK],
      prefetchStatus: { [ADA.id]: { state: "running", done: 12, total: 40, bytes: 3 * MIB } },
    });
    const { user } = renderApp();
    const panel = await openReadingSettings(user);

    const ada = await within(panel).findByTestId(`prefetch-status-${ADA.id}`);
    expect(await within(ada).findByText("Downloading: 12 of 40 (3 MB)")).toBeTruthy();
    expect(ada.textContent).toContain("Ada Lovelace (ada@vicemail.test)");
    const work = within(panel).getByTestId(`prefetch-status-${WORK.id}`);
    expect(await within(work).findByText("Up to date")).toBeTruthy();

    await emitCore({ type: "prefetchProgress", accountId: ADA.id, status: { state: "paused", done: 20, total: 40, bytes: 5 * MIB } });

    expect(await within(ada).findByText("Paused while offline")).toBeTruthy();
  });

  it("lists the senders allowed to load external content per account and removes one", async () => {
    const core = mockCore({
      accounts: [ADA, WORK],
      remoteSenders: { [ADA.id]: ["news@shop.example", "grace@example.org"] },
    });
    const { user } = renderApp();
    const panel = await openReadingSettings(user);

    const ada = await within(panel).findByTestId(`remote-senders-${ADA.id}`);
    expect(await within(ada).findByText("news@shop.example")).toBeTruthy();
    expect(within(ada).getByText("grace@example.org")).toBeTruthy();
    const work = within(panel).getByTestId(`remote-senders-${WORK.id}`);
    expect(await within(work).findByText(/No senders/)).toBeTruthy();
    expect(within(work).getByRole("heading").textContent).toBe("ada@work.test");

    await user.click(within(ada).getByRole("button", { name: "Block external content from news@shop.example again" }));

    expect(core.callsOf("revoke_remote_content")).toEqual([{ accountId: ADA.id, sender: "news@shop.example" }]);
    await waitFor(() => {
      expect(within(ada).queryByText("news@shop.example")).toBeNull();
    });
    expect(within(ada).getByText("grace@example.org")).toBeTruthy();
  });

  it("without an account there is nothing to list", async () => {
    mockCore();
    const { user } = renderApp();
    const panel = await openReadingSettings(user);
    expect(await within(panel).findByText("Appears once an account is set up.")).toBeTruthy();
    expect(within(panel).queryByRole("listitem")).toBeNull();
  });
});
