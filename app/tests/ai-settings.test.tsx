import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AI_PROVIDER_COMMANDS, CLOUD_PROFILE_ID, makeAiProfile, makeCloudProfile, makeRequestEntry, type AiFakeOptions } from "./support/ai";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { emitCore } from "./support/events";
import { renderApp } from "./support/render";

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "ada@work.test", displayName: "" });

function setup(ai: AiFakeOptions = {}, extra: Parameters<typeof mockCore>[0] = {}) {
  const core = mockCore({ accounts: [ADA, WORK], ai, ...extra });
  return { core, ...renderApp() };
}

async function openAi(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "AI in the app" }));
  const panel = screen.getByRole("tabpanel", { name: "AI in the app" });
  await within(panel).findByRole("switch", { name: "Use AI in the app" });
  return panel;
}

const providerCalls = (core: ReturnType<typeof mockCore>) => core.calls.filter((c) => AI_PROVIDER_COMMANDS.has(c.cmd));

describe("Settings, AI in the app", () => {
  it("is off until turned on, explains the difference to AI over MCP, and keeps the hints apart", async () => {
    const { core, user } = setup();
    const panel = await openAi(user);
    const toggle = within(panel).getByRole("switch", { name: "Use AI in the app" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(within(panel).getByTestId("ai-state").textContent).toBe("Off. No request leaves this computer.");
    expect(within(panel).queryByRole("region", { name: "Providers" })).toBeNull();
    // One sentence each for the two ways AI meets the mail.
    const difference = within(panel).getByTestId("ai-difference");
    expect(difference.textContent).toContain("AI in the app: this app sends the conversation");
    expect(difference.textContent).toContain("AI over MCP: an AI program of your own");
    // The open-reply hints work without AI.
    expect(within(panel).getByRole("region", { name: "Maybe still open" })).toBeTruthy();

    await user.click(toggle);
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    expect(core.callsOf("set_ai_enabled")).toEqual([{ enabled: true }]);
    expect(within(panel).getByRole("region", { name: "Providers" })).toBeTruthy();
    expect(within(panel).getByRole("region", { name: "Functions" })).toBeTruthy();
    expect(within(panel).getByRole("region", { name: "Cloud providers per account" })).toBeTruthy();
    expect(providerCalls(core)).toEqual([]);

    await user.click(within(panel).getByRole("button", { name: "Go to AI over MCP" }));
    expect(screen.getByRole("tab", { name: "AI over MCP" }).getAttribute("aria-selected")).toBe("true");
  });

  it("adds a local provider from a preset, then picks its model from the provider's list", async () => {
    const { core, user } = setup({ enabled: true, models: { "01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa": [{ id: "llama3.2", displayName: null }, { id: "qwen3", displayName: "Qwen 3" }] } });
    const panel = await openAi(user);
    expect(within(panel).getByText("No provider yet. Pick one below.")).toBeTruthy();
    await user.click(within(panel).getByRole("button", { name: "Add Ollama (Local)" }));
    const form = within(panel).getByRole("form", { name: "New provider" });
    expect(within(form).getByLabelText<HTMLInputElement>("Name").value).toBe("Ollama");
    expect(within(form).getByLabelText<HTMLInputElement>("Address").value).toBe("http://localhost:11434/v1");
    expect(within(form).getByText("Save the provider first, then choose a model from its list.")).toBeTruthy();
    await user.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(core.callsOf("save_ai_profile")).toHaveLength(1);
    });
    expect(core.callsOf("save_ai_profile")[0]).toEqual({
      profile: {
        id: null,
        name: "Ollama",
        kind: "openai_compatible",
        baseUrl: "http://localhost:11434/v1",
        model: "",
        structuredOutput: true,
        maxOutputTokens: null,
        key: { type: "keep" },
      },
    });
    // The new provider's form stays open for the model; the list is asked for only on request.
    const edit = await within(panel).findByRole("form", { name: "Edit Ollama" });
    expect(core.callsOf("list_ai_models")).toEqual([]);
    await user.click(within(edit).getByRole("button", { name: "Load models" }));
    const select = await within(edit).findByRole("combobox", { name: "Model" });
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["Choose a model", "llama3.2", "Qwen 3 (qwen3)"]);
    await user.selectOptions(select, "qwen3");
    await user.click(within(edit).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(core.ai.settings.profiles[0]?.model).toBe("qwen3");
    });
    const row = await within(panel).findByTestId("ai-profile-01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    expect(row.textContent).toContain("Local");
    expect(row.textContent).toContain("localhost:11434 · qwen3 · No key");
    expect(within(row).getByText("Local: no consent needed, nothing leaves this computer or your network.")).toBeTruthy();
  });

  it("sends a key once to the core and never shows it again", async () => {
    const { core, user } = setup({ enabled: true, profiles: [makeCloudProfile({ hasKey: false })] });
    const panel = await openAi(user);
    const row = within(panel).getByTestId(`ai-profile-${CLOUD_PROFILE_ID}`);
    await user.click(within(row).getByRole("button", { name: "Edit OpenAI" }));
    const form = within(panel).getByRole("form", { name: "Edit OpenAI" });
    const key = within(form).getByLabelText("API key");
    expect(key.getAttribute("type")).toBe("password");
    await user.type(key, "sk-secret-123");
    await user.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(core.ai.keysReceived).toEqual(["sk-secret-123"]);
    });
    expect((core.callsOf("save_ai_profile")[0]?.["profile"] as { key: unknown }).key).toEqual({ type: "set", key: "sk-secret-123" });
    const after = await within(panel).findByTestId(`ai-profile-${CLOUD_PROFILE_ID}`);
    expect(after.textContent).toContain("Key stored");
    expect(document.body.textContent).not.toContain("sk-secret-123");
    for (const input of document.querySelectorAll("input")) expect(input.value).not.toBe("sk-secret-123");
    // Only the save carried it.
    const withKey = core.calls.filter((c) => JSON.stringify(c.args ?? {}).includes("sk-secret-123"));
    expect(withKey.map((c) => c.cmd)).toEqual(["save_ai_profile"]);

    // Editing again keeps the key unless the user removes it.
    await user.click(within(after).getByRole("button", { name: "Edit OpenAI" }));
    const again = within(panel).getByRole("form", { name: "Edit OpenAI" });
    expect(within(again).getByLabelText<HTMLInputElement>("API key").value).toBe("");
    expect(within(again).getByText("A key is stored in the keychain. Leave the field empty to keep it.")).toBeTruthy();
    await user.click(within(again).getByRole("checkbox", { name: "Remove the stored key" }));
    await user.click(within(again).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect((core.callsOf("save_ai_profile")[1]?.["profile"] as { key: unknown }).key).toEqual({ type: "remove" });
    });
  });

  it("says before and after saving that another server removes the stored key, and asks for it again", async () => {
    const { core, user } = setup({ enabled: true, profiles: [makeCloudProfile({ needsConsent: false, consentAt: 1 })] });
    const panel = await openAi(user);
    const row = within(panel).getByTestId(`ai-profile-${CLOUD_PROFILE_ID}`);
    expect(within(row).queryByTestId("ai-key-removed")).toBeNull();
    await user.click(within(row).getByRole("button", { name: "Edit OpenAI" }));
    const form = within(panel).getByRole("form", { name: "Edit OpenAI" });
    const address = within(form).getByLabelText("Address");
    // Another path on the same server (written with its default port) keeps the key.
    await user.clear(address);
    await user.type(address, "https://API.openai.com:443/v2");
    expect(within(form).getByText("A key is stored in the keychain. Leave the field empty to keep it.")).toBeTruthy();
    // Another server: the form says what saving does to the key.
    await user.clear(address);
    await user.type(address, "https://api.openai.example/v1");
    expect(
      within(form).getByText(
        "The server changes: saving removes the stored key, so it never reaches the new server. Enter it again here if the new server needs one.",
      ),
    ).toBeTruthy();
    await user.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(core.callsOf("save_ai_profile")).toHaveLength(1);
    });
    expect((core.callsOf("save_ai_profile")[0]?.["profile"] as { key: unknown }).key).toEqual({ type: "keep" });

    // The core removed it; the provider says so and has no key.
    const after = await within(panel).findByTestId(`ai-profile-${CLOUD_PROFILE_ID}`);
    const notice = within(after).getByTestId("ai-key-removed");
    expect(notice.getAttribute("role")).toBe("status");
    expect(notice.textContent).toContain("Key removed: the server changed");
    expect(notice.textContent).toContain(
      "The API key was meant for the previous server, so it was removed from the keychain. If api.openai.example:443 needs a key, enter it again under Edit.",
    );
    expect(after.textContent).toContain("api.openai.example:443 · gpt-5 · No key");
    expect(providerCalls(core)).toEqual([]);

    // Editing asks for it again; entering it ends the notice.
    await user.click(within(after).getByRole("button", { name: "Edit OpenAI" }));
    const again = within(panel).getByRole("form", { name: "Edit OpenAI" });
    expect(within(again).getByText("The stored key was removed because the server changed. Enter it again if this server needs one.")).toBeTruthy();
    expect(within(again).queryByRole("checkbox", { name: "Remove the stored key" })).toBeNull();
    await user.type(within(again).getByLabelText("API key"), "sk-new-456");
    await user.click(within(again).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(core.ai.keysReceived).toEqual(["sk-new-456"]);
    });
    const done = await within(panel).findByTestId(`ai-profile-${CLOUD_PROFILE_ID}`);
    expect(done.textContent).toContain("api.openai.example:443 · gpt-5 · Key stored");
    expect(within(done).queryByTestId("ai-key-removed")).toBeNull();
  });

  it("checks the form before saving and shows the core's refusal at the form", async () => {
    const { core, user } = setup({ enabled: true });
    const panel = await openAi(user);
    await user.click(within(panel).getByRole("button", { name: "Other address" }));
    const form = within(panel).getByRole("form", { name: "New provider" });
    await user.click(within(form).getByRole("button", { name: "Save" }));
    expect(within(form).getByText("Enter a name.")).toBeTruthy();
    expect(within(form).getByText(/Enter a full address starting with https:\/\//)).toBeTruthy();
    expect(core.callsOf("save_ai_profile")).toEqual([]);

    await user.type(within(form).getByLabelText("Name"), "Remote box");
    await user.type(within(form).getByLabelText("Address"), "http://example.org/v1");
    await user.click(within(form).getByRole("button", { name: "Save" }));
    const alert = await within(form).findByRole("alert");
    expect(alert.getAttribute("data-error-code")).toBe("invalid_argument");
    expect(within(alert).getByText("The provider could not be saved.")).toBeTruthy();
  });

  it("chooses a provider or off per function, cloud permission per account (default no), and consent per cloud provider", async () => {
    const { core, user } = setup({ enabled: true, profiles: [makeAiProfile(), makeCloudProfile()] });
    const panel = await openAi(user);
    const functions = within(panel).getByRole("region", { name: "Functions" });
    const summary = within(functions).getByRole("combobox", { name: "Summaries" });
    expect((summary as HTMLSelectElement).value).toBe("");
    expect(within(summary).getAllByRole("option").map((o) => o.textContent)).toEqual(["Off", "Ollama (Local)", "OpenAI (Cloud)"]);
    await user.selectOptions(summary, CLOUD_PROFILE_ID);
    await waitFor(() => {
      expect(core.callsOf("set_ai_function")).toEqual([{ aiFunction: "summary", profileId: CLOUD_PROFILE_ID }]);
    });
    await user.selectOptions(within(functions).getByRole("combobox", { name: "Summaries" }), "");
    await waitFor(() => {
      expect(core.callsOf("set_ai_function")[1]).toEqual({ aiFunction: "summary", profileId: null });
    });

    const accounts = within(panel).getByRole("region", { name: "Cloud providers per account" });
    const ada = within(accounts).getByRole("switch", { name: "Ada Lovelace (ada@vicemail.test)" });
    const work = within(accounts).getByRole("switch", { name: "ada@work.test" });
    expect(ada.getAttribute("aria-checked")).toBe("false");
    expect(work.getAttribute("aria-checked")).toBe("false");
    await user.click(work);
    await waitFor(() => {
      expect(core.callsOf("set_ai_account_cloud")).toEqual([{ accountId: WORK.id, allowed: true }]);
    });

    const cloud = within(panel).getByTestId(`ai-profile-${CLOUD_PROFILE_ID}`);
    expect(within(cloud).getByText("Not agreed yet. You will be asked before the first request.")).toBeTruthy();
    await user.click(within(cloud).getByRole("button", { name: "Read what is sent to OpenAI and agree" }));
    const consent = within(cloud).getByRole("group", { name: "Before the first request to OpenAI" });
    expect(within(consent).getByTestId("ai-consent-to").textContent).toBe("To whom: OpenAI at api.openai.com:443, model gpt-5.");
    await user.click(within(consent).getByRole("button", { name: "Agree and send" }));
    await waitFor(() => {
      expect(core.callsOf("set_ai_consent")).toEqual([{ profileId: CLOUD_PROFILE_ID, granted: true }]);
    });
    expect(await within(cloud).findByText(/^You agreed on/)).toBeTruthy();
    await user.click(within(cloud).getByRole("button", { name: "Withdraw consent for OpenAI" }));
    await waitFor(() => {
      expect(core.callsOf("set_ai_consent")[1]).toEqual({ profileId: CLOUD_PROFILE_ID, granted: false });
    });
    expect(providerCalls(core)).toEqual([]);
  });

  it("tests a connection without mail and deletes a provider after asking", async () => {
    const { core, user } = setup({
      enabled: true,
      profiles: [makeAiProfile()],
      connection: { models: 3, modelListed: true, chatOk: false, chatError: "ai_unreachable" },
    });
    const panel = await openAi(user);
    const row = within(panel).getByTestId("ai-profile-0123456789abcdef0123456789abcdef");
    await user.click(within(row).getByRole("button", { name: "Test connection" }));
    const result = await within(row).findByTestId("ai-test-result");
    expect(result.textContent).toContain("The provider answered and lists 3 models.");
    expect(result.textContent).toContain("The test request failed: The AI provider could not be reached.");
    expect(core.callsOf("test_ai_connection")).toEqual([{ profileId: "0123456789abcdef0123456789abcdef" }]);

    await user.click(within(row).getByRole("button", { name: "Delete Ollama" }));
    const ask = within(row).getByRole("group", { name: "Delete Ollama?" });
    expect(ask.textContent).toContain("its key is removed from the keychain");
    await user.click(within(ask).getByRole("button", { name: "Delete provider" }));
    await waitFor(() => {
      expect(core.callsOf("delete_ai_profile")).toEqual([{ profileId: "0123456789abcdef0123456789abcdef" }]);
    });
    expect(await within(panel).findByText("No provider yet. Pick one below.")).toBeTruthy();
  });

  it("switches the open-reply hints and their days, also with AI off", async () => {
    const { core, user } = setup({ enabled: false, openReplies: { enabled: true, days: 3 } });
    const panel = await openAi(user);
    const hints = within(panel).getByRole("region", { name: "Maybe still open" });
    const days = within(hints).getByLabelText("Days until a question you sent counts as unanswered");
    await user.clear(days);
    await user.type(days, "45");
    expect(within(hints).getByText("Enter a number from 1 to 30.")).toBeTruthy();
    await user.clear(days);
    await user.type(days, "7");
    await user.tab();
    await waitFor(() => {
      expect(core.callsOf("set_open_reply_settings")).toEqual([{ settings: { enabled: true, days: 7 } }]);
    });
    await user.click(within(hints).getByRole("switch", { name: "Show hints at open replies" }));
    await waitFor(() => {
      expect(core.callsOf("set_open_reply_settings")[1]).toEqual({ settings: { enabled: false, days: 7 } });
    });
    expect(providerCalls(core)).toEqual([]);
  });

  it("shows the request log with metadata only, and reloads it when a request finished", async () => {
    const { core, user } = setup({
      enabled: true,
      log: [
        makeRequestEntry({ id: 2, function: "search", profileName: "OpenAI", host: "api.openai.com:443", locality: "internet", model: "gpt-5", mails: 4, outcome: "failed", error: "ai_busy" }),
        makeRequestEntry({ id: 1 }),
      ],
    });
    const panel = await openAi(user);
    await user.click(within(panel).getByRole("tab", { name: "Request log" }));
    const table = await within(panel).findByRole("table", { name: "AI requests, newest first" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain("Questions about your mail");
    expect(rows[0]?.textContent).toContain("OpenAI");
    expect(rows[0]?.textContent).toContain("Cloud");
    expect(rows[0]?.textContent).toContain("api.openai.com:443 · gpt-5");
    expect(rows[0]?.textContent).toContain("4 mails");
    expect(rows[0]?.textContent).toContain("Failed");
    expect(rows[0]?.textContent).toContain("The AI provider is busy right now.");
    expect(rows[1]?.textContent).toContain("Summaries");
    expect(rows[1]?.textContent).toContain("Done");
    expect(rows[1]?.textContent).toContain("2.4 s");

    const before = core.callsOf("list_ai_requests").length;
    await emitCore({ type: "aiRequest", requestId: "r1", phase: { phase: "finished", outcome: "ok" } });
    await waitFor(() => {
      expect(core.callsOf("list_ai_requests").length).toBeGreaterThan(before);
    });
  });

  it("says so where the settings cannot be loaded or changed", async () => {
    const { user } = setup({}, { failures: { set_ai_enabled: ipcError("database") } });
    const panel = await openAi(user);
    await user.click(within(panel).getByRole("switch", { name: "Use AI in the app" }));
    const alert = await within(panel).findByRole("alert");
    expect(within(alert).getByText("The switch could not be changed.")).toBeTruthy();
    expect(within(alert).getByText("The local data could not be read or written.")).toBeTruthy();
  });
});
