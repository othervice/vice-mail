import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { emptyRule, moved, opsFor, ruleProblem } from "../src/features/comfort/rules";
import type { MailRule } from "../src/services/ipc";
import { makeAccount, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

const ADA = makeAccount();

describe("local rules: checks before the core is asked", () => {
  it("names every problem the core would reject", () => {
    const valid = { ...emptyRule(), name: "Receipts", actions: [{ type: "markRead" as const }] };
    expect(ruleProblem(valid)).toBeNull();
    expect(ruleProblem({ ...valid, name: "  " })).toBe("name");
    expect(ruleProblem({ ...valid, name: "x".repeat(201) })).toBe("tooLong");
    expect(ruleProblem({ ...valid, actions: [] })).toBe("noAction");
    expect(ruleProblem({ ...valid, conditions: [{ field: "from", op: "contains", value: " " }] })).toBe("emptyValue");
    expect(ruleProblem({ ...valid, conditions: [{ field: "account", accountId: "" }] })).toBe("noAccount");
    expect(ruleProblem({ ...valid, actions: [{ type: "move", accountId: ADA.id, path: "" }] })).toBe("noFolder");
    expect(
      ruleProblem({ ...valid, conditions: Array.from({ length: 21 }, () => ({ field: "subject" as const, op: "is" as const, value: "x" })) }),
    ).toBe("tooManyParts");
  });

  it("offers the domain comparison only for addresses", () => {
    expect(opsFor("from")).toEqual(["contains", "is", "domain"]);
    expect(opsFor("to")).toEqual(["contains", "is", "domain"]);
    expect(opsFor("subject")).toEqual(["contains", "is"]);
    expect(opsFor("list")).toEqual(["contains", "is"]);
  });

  it("moves a rule up or down, never past the ends", () => {
    expect(moved(["a", "b", "c"], 1, -1)).toEqual(["b", "a", "c"]);
    expect(moved(["a", "b", "c"], 1, 1)).toEqual(["a", "c", "b"]);
    expect(moved(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(moved(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
  });
});

function rule(id: string, name: string, position: number): MailRule {
  return {
    id,
    name,
    enabled: true,
    matchAll: true,
    conditions: [{ field: "from", op: "domain", value: "shop.example" }],
    actions: [{ type: "markRead" }],
    stop: false,
    position,
  };
}

async function openInboxSettings(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "Inbox" }));
  return screen.findByRole("region", { name: "Local rules" });
}

describe("local rules in the settings", () => {
  it("are labelled as local and say so when there are none", async () => {
    mockCore({ accounts: [ADA], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    const section = await openInboxSettings(user);
    expect(within(section).getByTestId("rules-local-note").textContent).toContain("only while the app is running");
    expect(await within(section).findByTestId("rules-empty")).toBeTruthy();
  });

  it("a new rule with a condition and a move is checked, then created as entered", async () => {
    const core = mockCore({ accounts: [ADA], mailboxes: standardMailboxes() });
    const { user } = renderApp();
    const section = await openInboxSettings(user);
    await user.click(await within(section).findByTestId("rule-new"));
    const editor = within(section).getByTestId("rule-editor");

    await user.click(within(editor).getByTestId("rule-save"));
    expect(within(editor).getByText("Give the rule a name.")).toBeTruthy();
    expect(core.callsOf("create_rule")).toHaveLength(0);

    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Receipts");
    await user.click(within(editor).getByTestId("rule-add-condition"));
    await user.selectOptions(within(editor).getByTestId("rule-condition-op"), "domain");
    await user.type(within(editor).getByTestId("rule-condition-value"), "shop.example");
    await user.click(within(editor).getByTestId("rule-add-action"));
    await user.selectOptions(within(editor).getByTestId("rule-action-type"), "move");

    await user.click(within(editor).getByTestId("rule-save"));
    expect(within(editor).getByTestId("rule-problem").textContent).toBe("Choose the folder to move to.");

    await waitFor(() => {
      expect(within(within(editor).getByTestId("rule-move-folder")).getAllByRole("option").length).toBeGreaterThan(1);
    });
    await user.selectOptions(within(editor).getByTestId("rule-move-folder"), "Projekte/2026");
    await user.click(within(editor).getByTestId("rule-save"));

    await waitFor(() => {
      expect(core.callsOf("create_rule")).toEqual([
        {
          input: {
            name: "Receipts",
            enabled: true,
            matchAll: true,
            conditions: [{ field: "from", op: "domain", value: "shop.example" }],
            actions: [{ type: "move", accountId: ADA.id, path: "Projekte/2026" }],
            stop: false,
          },
        },
      ]);
    });
    expect(await within(section).findByRole("switch", { name: "Receipts" })).toBeTruthy();
    expect(within(section).queryByTestId("rule-editor")).toBeNull();
  });

  it("switches, reorders and deletes rules", async () => {
    const core = mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      comfort: { rules: [rule("r1", "First", 0), rule("r2", "Second", 1)] },
    });
    const { user } = renderApp();
    const section = await openInboxSettings(user);

    await user.click(await within(section).findByRole("switch", { name: "First" }));
    await waitFor(() => {
      expect(core.callsOf("update_rule")[0]).toMatchObject({ id: "r1", input: { name: "First", enabled: false } });
    });

    await user.click(within(section).getByRole("button", { name: "Move “Second” up" }));
    await waitFor(() => {
      expect(core.callsOf("reorder_rules")).toEqual([{ ids: ["r2", "r1"] }]);
    });
    await waitFor(() => {
      const labelOf = (s: HTMLElement) => document.getElementById(s.getAttribute("aria-labelledby") ?? "")?.textContent;
      expect(within(section).getAllByRole("switch").map(labelOf)).toEqual([
        "Second",
        "First",
      ]);
    });

    await user.click(within(section).getByRole("button", { name: "Delete “First”" }));
    await waitFor(() => {
      expect(core.callsOf("delete_rule")).toEqual([{ id: "r1" }]);
    });
    await waitFor(() => {
      expect(within(section).getAllByTestId("rule")).toHaveLength(1);
    });
  });

  it("a rule the core rejects keeps the form open and says why", async () => {
    mockCore({
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      failures: { create_rule: { code: "invalid_argument", message: "x", detail: "too many rules" } },
    });
    const { user } = renderApp();
    const section = await openInboxSettings(user);
    await user.click(await within(section).findByTestId("rule-new"));
    const editor = within(section).getByTestId("rule-editor");
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "All read");
    await user.click(within(editor).getByTestId("rule-add-action"));
    await user.click(within(editor).getByTestId("rule-save"));
    expect(await within(editor).findByText("The rule could not be saved.")).toBeTruthy();
    expect(within(section).getByTestId("rule-editor")).toBeTruthy();
  });
});
