import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ipcError, makeAccount, mockCore } from "./support/core";
import { emitCore } from "./support/events";
import { makeAccessEntry, makeMcpClient, MCP_ENDPOINT, MCP_HELPER_PATH } from "./support/mcpSync";
import { renderApp } from "./support/render";

const ADA = makeAccount();
const WORK = makeAccount({ id: "33333333-3333-4333-8333-333333333333", email: "ada@work.test", displayName: "", writesEnabled: true });

async function openMcp(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "AI over MCP" }));
  const panel = screen.getByRole("tabpanel", { name: "AI over MCP" });
  await within(panel).findByRole("switch", { name: "Allow AI clients" });
  return panel;
}

async function openAccess(user: ReturnType<typeof renderApp>["user"]) {
  const panel = await openMcp(user);
  await user.click(within(panel).getByRole("tab", { name: "Access" }));
  return panel;
}

describe("Settings, AI over MCP", () => {
  it("is off until turned on, and explains first what a cloud client does with the mail", async () => {
    const core = mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const panel = await openMcp(user);

    const toggle = within(panel).getByRole("switch", { name: "Allow AI clients" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(within(panel).getByTestId("mcp-state").textContent).toBe("Off. No AI client can connect.");
    expect(within(panel).queryByTestId("mcp-stop")).toBeNull();

    await user.click(toggle);
    const ask = within(panel).getByRole("group", { name: "Before you turn on AI access" });
    expect(ask.textContent).toContain("sends the mails it reads to its provider. The app cannot prevent that");
    expect(document.activeElement).toBe(ask);
    expect(core.callsOf("set_mcp_enabled")).toEqual([]);

    await user.click(within(ask).getByRole("button", { name: "Cancel" }));
    expect(within(panel).queryByRole("group", { name: "Before you turn on AI access" })).toBeNull();
    expect(core.callsOf("set_mcp_enabled")).toEqual([]);
    expect(core.mcpSync.mcpEnabled).toBe(false);

    await user.click(toggle);
    await user.click(within(panel).getByRole("button", { name: "Turn on AI access" }));
    await waitFor(() => {
      expect(toggle.getAttribute("aria-checked")).toBe("true");
    });
    expect(core.callsOf("set_mcp_enabled")).toEqual([{ enabled: true }]);
    expect(within(panel).getByTestId("mcp-state").textContent).toBe("On. Waiting for AI clients on this computer.");
    expect(within(panel).getByText(/a cloud AI client sends what it reads to its provider/)).toBeTruthy();
    expect(within(panel).getByText(MCP_ENDPOINT)).toBeTruthy();
    expect(within(panel).getByText(MCP_HELPER_PATH)).toBeTruthy();
  });

  it("stops all access with the emergency stop, also from the access view", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { enabled: true } } });
    const { user } = renderApp();
    const panel = await openMcp(user);

    await user.click(within(panel).getByRole("button", { name: "Stop all AI access now" }));
    await waitFor(() => {
      expect(within(panel).getByRole("switch", { name: "Allow AI clients" }).getAttribute("aria-checked")).toBe("false");
    });
    expect(core.callsOf("set_mcp_enabled")).toEqual([{ enabled: false }]);
    expect(within(panel).queryByRole("button", { name: "Stop all AI access now" })).toBeNull();

    // Turned on again elsewhere (another window): the access view offers the stop as well.
    await emitCore({ type: "mcpStatusChanged", status: mcpOnStatus() });
    await user.click(within(panel).getByRole("tab", { name: "Access" }));
    const stop = await within(panel).findByRole("button", { name: "Stop all AI access now" });
    await user.click(stop);
    await waitFor(() => {
      expect(within(panel).getByTestId("mcp-access-off").textContent).toBe("AI access is off. No client can connect.");
    });
    expect(core.callsOf("set_mcp_enabled")).toEqual([{ enabled: false }, { enabled: false }]);
  });

  it("turns off with the switch at once, without a question", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { enabled: true } } });
    const { user } = renderApp();
    const panel = await openMcp(user);
    await user.click(within(panel).getByRole("switch", { name: "Allow AI clients" }));
    await waitFor(() => {
      expect(core.mcpSync.mcpEnabled).toBe(false);
    });
    expect(within(panel).queryByRole("group")).toBeNull();
  });

  it("follows mcpStatusChanged, including a failed endpoint", async () => {
    mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const panel = await openMcp(user);

    await emitCore({
      type: "mcpStatusChanged",
      status: { ...mcpOnStatus(), state: "failed", endpoint: null, error: "bind /tmp/x: permission denied" },
    });
    await waitFor(() => {
      expect(within(panel).getByRole("switch", { name: "Allow AI clients" }).getAttribute("aria-checked")).toBe("true");
    });
    const alert = within(panel).getByRole("alert");
    expect(alert.textContent).toContain("On, but the app could not open its connection point.");
    expect(alert.textContent).toContain("bind /tmp/x: permission denied");
  });

  it("asks to move the app first when it runs from a disk image, and says when the helper is missing", async () => {
    mockCore({
      accounts: [ADA],
      mcpSync: { mcp: { enabled: true, status: { helper: { path: null, found: false, unstableLocation: true } } } },
    });
    const { user } = renderApp();
    const panel = await openMcp(user);
    expect(within(panel).getByTestId("mcp-unstable").textContent).toContain("Move the app to the Applications folder first");
    expect(within(panel).getByTestId("mcp-helper-missing").textContent).toContain("The helper program is missing");
  });

  it("shares each account at one of three levels and says where drafts of a read-only account stay", async () => {
    const core = mockCore({ accounts: [ADA, WORK] });
    const { user } = renderApp();
    const panel = await openMcp(user);

    const ada = within(panel).getByRole("radiogroup", { name: "Ada Lovelace (ada@vicemail.test)" });
    const work = within(panel).getByRole("radiogroup", { name: "ada@work.test" });
    expect(within(ada).getAllByRole("radio").map((radio) => radio.textContent)).toEqual(["Invisible", "Read", "Read and drafts"]);
    expect(within(ada).getByRole("radio", { name: "Invisible" }).getAttribute("aria-checked")).toBe("true");
    expect(within(panel).getAllByText("AI clients do not see this account, not even in search.")).toHaveLength(2);

    await user.click(within(ada).getByRole("radio", { name: "Read and drafts" }));
    await waitFor(() => {
      expect(within(ada).getByRole("radio", { name: "Read and drafts" }).getAttribute("aria-checked")).toBe("true");
    });
    expect(core.callsOf("set_mcp_account_access")).toEqual([{ accountId: ADA.id, access: "read_and_drafts" }]);
    expect(within(panel).getByTestId("mcp-local-drafts").textContent).toContain("stay in the app on this computer");

    await user.click(within(work).getByRole("radio", { name: "Read and drafts" }));
    await waitFor(() => {
      expect(core.mcpSync.shares.get(WORK.id)).toBe("read_and_drafts");
    });
    // Changes are on for the work account: its drafts go to the server, no note.
    expect(within(panel).getAllByTestId("mcp-local-drafts")).toHaveLength(1);

    await user.click(within(work).getByRole("radio", { name: "Read" }));
    await waitFor(() => {
      expect(within(work).getByRole("radio", { name: "Read" }).getAttribute("aria-checked")).toBe("true");
    });
    expect(within(panel).getByText("AI clients can list, search and read its mail. Nothing becomes read.")).toBeTruthy();
  });

  it("shows a refused share change where it happened", async () => {
    mockCore({ accounts: [ADA], failures: { set_mcp_account_access: ipcError("database") } });
    const { user } = renderApp();
    const panel = await openMcp(user);
    await user.click(within(panel).getByRole("radio", { name: "Read" }));
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("The share could not be changed.");
    expect(alert.dataset["errorCode"]).toBe("database");
  });

  it("says there is nothing to share without accounts", async () => {
    mockCore({ accounts: [] });
    const { user } = renderApp();
    const panel = await openMcp(user);
    expect(within(panel).getByText("No accounts yet. Add an account first, then choose what AI clients may see.")).toBeTruthy();
    expect(within(panel).getByText("No AI clients yet. Add one to get a configuration to copy.")).toBeTruthy();
  });

  it("adds a client, shows its key once inside the configurations, copies one and forgets the key", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { enabled: true } } });
    const { user, queryClient } = renderApp();
    const panel = await openMcp(user);

    await user.click(within(panel).getByRole("button", { name: "Add AI client" }));
    expect(within(panel).getByText("Enter a name.")).toBeTruthy();
    expect(core.callsOf("create_mcp_client")).toEqual([]);

    await user.type(within(panel).getByLabelText("Name of the new AI client"), "  Claude on the MacBook ");
    await user.click(within(panel).getByRole("button", { name: "Add AI client" }));

    const created = await within(panel).findByTestId("mcp-created");
    expect(core.callsOf("create_mcp_client")).toEqual([{ name: "Claude on the MacBook" }]);
    expect(created.textContent).toContain("Claude on the MacBook is ready");
    expect(created.textContent).toContain("is shown only now");
    const desktop = within(created).getByTestId("mcp-config-content-claude_desktop");
    expect(desktop.textContent).toContain("s3cr3tkey");
    expect(within(created).getByTestId("mcp-config-location").textContent).toBe(
      "Where it goes: ~/Library/Application Support/Claude/claude_desktop_config.json",
    );

    await user.click(within(created).getByRole("tab", { name: "Claude Code" }));
    const code = within(created).getByTestId("mcp-config-content-claude_code");
    expect(code.textContent).toMatch(/^claude mcp add --transport stdio --scope user /);
    expect(within(created).getByTestId("mcp-config-location").textContent).toBe("Run this command in a terminal.");
    await user.click(within(created).getByRole("button", { name: "Copy the configuration for Claude Code" }));
    expect(await navigator.clipboard.readText()).toBe(code.textContent);
    expect(within(created).getByText("Copied.")).toBeTruthy();

    // The new client is listed while its key is still shown.
    const list = within(panel).getByRole("list", { name: "AI clients" });
    expect(within(list).getByText("Claude on the MacBook")).toBeTruthy();
    expect(within(list).getByText(/never used/)).toBeTruthy();

    await user.click(within(created).getByRole("button", { name: "Done, hide the key" }));
    expect(within(panel).queryByTestId("mcp-created")).toBeNull();
    expect(document.body.textContent).not.toContain("s3cr3tkey");
    // Nothing else keeps it: the mutation that carried it is gone from the cache too.
    await waitFor(() => {
      expect(JSON.stringify(queryClient.getMutationCache().getAll().map((m) => m.state))).not.toContain("s3cr3tkey");
    });
    expect(within(panel).getByLabelText<HTMLInputElement>("Name of the new AI client").value).toBe("");
  });

  it("says when copying did not work", async () => {
    mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const panel = await openMcp(user);
    const writeText = navigator.clipboard.writeText.bind(navigator.clipboard);
    navigator.clipboard.writeText = () => Promise.reject(new Error("denied"));
    try {
      await user.click(await within(panel).findByRole("button", { name: "Copy the configuration for Claude Desktop" }));
      expect(await within(panel).findByText("Copying did not work. Select the text and copy it by hand.")).toBeTruthy();
    } finally {
      navigator.clipboard.writeText = writeText;
    }
  });

  it("shows why a client could not be added", async () => {
    mockCore({ accounts: [ADA], failures: { create_mcp_client: ipcError("keychain") } });
    const { user } = renderApp();
    const panel = await openMcp(user);
    await user.type(within(panel).getByLabelText("Name of the new AI client"), "Cursor");
    await user.click(within(panel).getByRole("button", { name: "Add AI client" }));
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("The AI client could not be added.");
    expect(alert.dataset["errorCode"]).toBe("keychain");
  });

  it("revokes a client at once and keeps revoked ones listed", async () => {
    const active = makeMcpClient({ lastUsedAt: Date.UTC(2026, 10, 22, 12, 0) });
    const old = makeMcpClient({ clientId: "f".repeat(32), name: "Old Cursor", revokedAt: Date.UTC(2026, 10, 21, 9, 0) });
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { enabled: true, clients: [active, old] } } });
    const { user } = renderApp();
    const panel = await openMcp(user);

    const oldRow = within(panel).getByTestId(`mcp-client-${old.clientId}`);
    expect(within(oldRow).getByText("Revoked")).toBeTruthy();
    expect(within(oldRow).queryByRole("button")).toBeNull();
    const row = within(panel).getByTestId(`mcp-client-${active.clientId}`);
    expect(row.textContent).toContain("last used");

    await user.click(within(row).getByRole("button", { name: "Revoke Claude Desktop" }));
    await waitFor(() => {
      expect(within(row).getByText("Revoked")).toBeTruthy();
    });
    expect(core.callsOf("revoke_mcp_client")).toEqual([{ clientId: active.clientId }]);
    expect(within(row).queryByRole("button")).toBeNull();
  });

  it("shows the setup help with the placeholder and tests the connection", async () => {
    const core = mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const panel = await openMcp(user);

    const content = await within(panel).findByTestId("mcp-config-content-claude_desktop");
    expect(content.textContent).toContain("<token>");
    expect(within(panel).getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Settings",
      "Access",
      "Claude Desktop",
      "Claude Code",
      "Cursor",
      "LM Studio",
    ]);
    await user.click(within(panel).getByRole("tab", { name: "LM Studio" }));
    expect(within(panel).getByTestId("mcp-config-location").textContent).toBe(
      "Where it goes: LM Studio: Program > Install > Edit mcp.json",
    );

    await user.click(within(panel).getByRole("button", { name: "Test connection" }));
    const problem = await within(panel).findByTestId("mcp-test-problem");
    expect(problem.textContent).toContain("AI access is off. Turn it on first.");

    await user.click(within(panel).getByRole("radio", { name: "Read" }));
    await waitFor(() => {
      expect(core.mcpSync.shares.get(ADA.id)).toBe("read");
    });
    await user.click(within(panel).getByRole("switch", { name: "Allow AI clients" }));
    await user.click(within(panel).getByRole("button", { name: "Turn on AI access" }));
    await waitFor(() => {
      expect(core.mcpSync.mcpEnabled).toBe(true);
    });
    await user.click(within(panel).getByRole("button", { name: "Test connection" }));
    const ok = await within(panel).findByTestId("mcp-test-ok");
    expect(ok.textContent).toContain("AI clients see 1 account.");
    expect(ok.textContent).toContain("Tools: List accounts, Search mail");
    expect(core.callsOf("test_mcp_connection")).toHaveLength(2);
  });

  it("reports a refused test connection with its reason and the technical detail", async () => {
    mockCore({
      accounts: [ADA],
      mcpSync: {
        mcp: {
          enabled: true,
          test: {
            ok: false,
            problem: "refused",
            refusal: "bad_proof",
            helperPath: MCP_HELPER_PATH,
            protocolVersion: null,
            tools: [],
            sharedAccounts: 0,
            detail: "handshake refused: bad proof",
            durationMs: 40,
          },
        },
      },
    });
    const { user } = renderApp();
    const panel = await openMcp(user);
    await user.click(within(panel).getByRole("button", { name: "Test connection" }));
    const problem = await within(panel).findByTestId("mcp-test-problem");
    expect(problem.textContent).toContain("The app refused the helper program.");
    expect(problem.textContent).toContain("Wrong key");
    expect(problem.textContent).toContain("handshake refused: bad proof");
  });

  it("shows a settings load failure with a retry", async () => {
    mockCore({ accounts: [ADA], failures: { get_mcp_settings: ipcError("database") } });
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "AI over MCP" }));
    const alert = await within(screen.getByRole("tabpanel", { name: "AI over MCP" })).findByRole("alert");
    expect(alert.textContent).toContain("The AI settings could not be loaded.");
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeTruthy();
  });
});

describe("AI over MCP and settings sync in German", () => {
  it("speak German with du", async () => {
    mockCore({ accounts: [ADA], preferences: { theme: "system", language: "de" } });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("button", { name: "Einstellungen" }));
    await user.click(screen.getByRole("tab", { name: "KI über MCP" }));
    const panel = screen.getByRole("tabpanel", { name: "KI über MCP" });
    await user.click(await within(panel).findByRole("switch", { name: "KI-Clients erlauben" }));
    const ask = within(panel).getByRole("group", { name: "Bevor du den KI-Zugriff einschaltest" });
    expect(ask.textContent).toContain("Die App kann das nicht verhindern");
    expect(within(panel).getByRole("tab", { name: "Zugriffe" })).toBeTruthy();

    await user.click(screen.getByRole("tab", { name: "Einstellungs-Sync" }));
    const sync = screen.getByRole("tabpanel", { name: "Einstellungs-Sync" });
    expect(await within(sync).findByRole("radiogroup", { name: "Sync-Konto" })).toBeTruthy();
    expect(within(sync).getByText(/Ein paar Wörter, die nichts miteinander zu tun haben/)).toBeTruthy();
  });
});

describe("AI over MCP, access view", () => {
  it("lists every access with client, tool, mails, time and result", async () => {
    mockCore({
      accounts: [ADA],
      mcpSync: {
        mcp: {
          enabled: true,
          access: [
            makeAccessEntry({ id: 3, tool: "create_draft", messages: 1, outcome: "refused" }),
            makeAccessEntry({ id: 2, tool: null, clientId: null, clientName: null, messages: 0, outcome: "refused", refusal: "unknown_client" }),
            makeAccessEntry({ id: 1, tool: "read_thread", messages: 4, outcome: "ok" }),
          ],
        },
      },
    });
    const { user } = renderApp();
    const panel = await openAccess(user);

    const table = await within(panel).findByRole("table", { name: "Access by AI clients, newest first" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows.map((row) => within(row).getAllByRole("cell").slice(1).map((cell) => cell.textContent))).toEqual([
      ["Claude Desktop", "Create a draft", "1 mail", "Refused"],
      ["Unknown client", "Connection refused: Unknown or revoked client", "0 mails", "Refused"],
      ["Claude Desktop", "Read a conversation", "4 mails", "Done"],
    ]);
    expect(within(rows[0] as HTMLElement).getByRole("cell", { name: /2026/ }).querySelector("time")).toBeTruthy();
    expect(within(panel).getByRole("button", { name: "Stop all AI access now" })).toBeTruthy();
  });

  it("adds new access live from mcpAccess, once", async () => {
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { enabled: true, access: [makeAccessEntry({ id: 1 })] } } });
    const { user } = renderApp();
    const panel = await openAccess(user);
    await within(panel).findByTestId("mcp-access-1");

    const fresh = [makeAccessEntry({ id: 2, tool: "list_accounts", messages: 0 }), makeAccessEntry({ id: 3, tool: "read_message", messages: 1 })];
    core.mcpSync.addAccess(fresh);
    await emitCore({ type: "mcpAccess", entries: fresh });
    await emitCore({ type: "mcpAccess", entries: fresh });

    await within(panel).findByTestId("mcp-access-3");
    const ids = within(panel)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.dataset["testid"]);
    expect(ids).toEqual(["mcp-access-3", "mcp-access-2", "mcp-access-1"]);
  });

  it("pages through older entries", async () => {
    const access = Array.from({ length: 150 }, (_, i) => makeAccessEntry({ id: 150 - i }));
    const core = mockCore({ accounts: [ADA], mcpSync: { mcp: { access } } });
    const { user } = renderApp();
    const panel = await openAccess(user);
    await within(panel).findByTestId("mcp-access-150");
    expect(within(panel).getAllByRole("row")).toHaveLength(101);
    expect(within(panel).getByTestId("mcp-access-off")).toBeTruthy();

    await user.click(within(panel).getByRole("button", { name: "Show older" }));
    await within(panel).findByTestId("mcp-access-1");
    expect(within(panel).getAllByRole("row")).toHaveLength(151);
    expect(within(panel).queryByRole("button", { name: "Show older" })).toBeNull();
    expect(core.callsOf("list_mcp_access")).toEqual([
      { before: null, limit: 100 },
      { before: 51, limit: 100 },
    ]);
  });

  it("has an empty state and shows a load failure", async () => {
    mockCore({ accounts: [ADA] });
    const { user } = renderApp();
    const panel = await openAccess(user);
    expect(await within(panel).findByText("No access yet")).toBeTruthy();
  });

  it("shows a failed access log with a retry", async () => {
    mockCore({ accounts: [ADA], failures: { list_mcp_access: ipcError("database") } });
    const { user } = renderApp();
    const panel = await openAccess(user);
    const alert = await within(panel).findByRole("alert");
    expect(alert.textContent).toContain("The access log could not be loaded.");
  });
});

function mcpOnStatus() {
  return {
    enabled: true,
    state: "listening" as const,
    endpoint: MCP_ENDPOINT,
    error: null,
    helper: { path: MCP_HELPER_PATH, found: true, unstableLocation: false },
  };
}
