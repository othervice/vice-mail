import { describe, expect, it } from "vitest";
import { isUserTag, isValidKeyword, tagsOf } from "../src/features/actions/keywords";
import { newFolderPath } from "../src/features/actions/MissingMailboxPrompt";
import {
  ACTION_IDS,
  ACTIONS,
  actionState,
  leavesFolder,
  QUICK_ACTIONS,
  requiredRole,
  TOOLBAR_ACTIONS,
  type ActionContext,
} from "../src/features/actions/registry";
import { ariaShortcut, formatShortcut, isEditableTarget, matchesShortcut } from "../src/features/actions/shortcuts";
import { en } from "../src/i18n/en";
import type { MailboxRole } from "../src/services/ipc";
import { makeAccount, makeMailbox, makeMessage } from "./support/core";

const key = (k: string, mods: Partial<Record<"shiftKey" | "metaKey" | "ctrlKey" | "altKey", boolean>> = {}) => ({
  key: k,
  shiftKey: false,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

function ctx(role: MailboxRole, overrides: Partial<ActionContext> = {}): ActionContext {
  return {
    account: makeAccount({ writesEnabled: true }),
    mailbox: makeMailbox({ id: 1, path: role === "inbox" ? "INBOX" : role, role }),
    items: [makeMessage({ unread: true })],
    canUndo: false,
    ...overrides,
  };
}

describe("action directory", () => {
  it("every action has a label in both languages and a definition under its id", () => {
    for (const id of ACTION_IDS) {
      const definition = ACTIONS[id];
      expect(definition.id).toBe(id);
      const label = definition.label(ctx("inbox"));
      expect(en.actions.labels).toHaveProperty(label);
    }
    for (const id of [...QUICK_ACTIONS, ...TOOLBAR_ACTIONS]) expect(ACTION_IDS).toContain(id);
  });

  it.each(["macos", "windows"] as const)("no two actions share a shortcut where both are available (%s)", (platform) => {
    const roles: MailboxRole[] = ["inbox", "archive", "trash", "junk", "sent", "custom"];
    for (const role of roles) {
      const context = ctx(role, { canUndo: true, items: [makeMessage({ unread: true }), makeMessage({ unread: false, flagged: true })] });
      const taken = new Map<string, string>();
      for (const id of ACTION_IDS) {
        const definition = ACTIONS[id];
        if (actionState(definition, context).kind === "unavailable") continue;
        for (const shortcut of definition.shortcuts[platform]) {
          const name = ariaShortcut(shortcut, platform);
          expect(taken.get(name), `${name} in ${role}: ${id} and ${taken.get(name) ?? ""}`).toBeUndefined();
          taken.set(name, id);
        }
      }
    }
  });

  it("offers delete in ordinary folders and delete permanently only in the trash", () => {
    expect(actionState(ACTIONS.delete, ctx("inbox")).kind).toBe("enabled");
    expect(actionState(ACTIONS.deletePermanently, ctx("inbox")).kind).toBe("unavailable");
    expect(actionState(ACTIONS.delete, ctx("trash")).kind).toBe("unavailable");
    expect(actionState(ACTIONS.deletePermanently, ctx("trash")).kind).toBe("enabled");
    expect(actionState(ACTIONS.archive, ctx("archive")).kind).toBe("unavailable");
    expect(actionState(ACTIONS.spam, ctx("junk")).kind).toBe("unavailable");
    expect(actionState(ACTIONS.notSpam, ctx("junk")).kind).toBe("enabled");
    expect(actionState(ACTIONS.notSpam, ctx("inbox")).kind).toBe("unavailable");
  });

  it("disables writing actions on read-only accounts, with the reason, but not selecting", () => {
    const readOnly = ctx("inbox", { account: makeAccount({ writesEnabled: false }) });
    for (const id of ["archive", "delete", "toggleRead", "toggleFlag", "move", "tags", "spam"] as const) {
      expect(actionState(ACTIONS[id], readOnly).kind).toBe("readOnly");
    }
    expect(actionState(ACTIONS.toggleSelect, readOnly).kind).toBe("enabled");
    expect(actionState(ACTIONS.selectAll, readOnly).kind).toBe("enabled");
  });

  it("toggles name what they would do now", () => {
    const unread = ctx("inbox", { items: [makeMessage({ unread: true, flagged: false })] });
    const read = ctx("inbox", { items: [makeMessage({ unread: false, flagged: true })] });
    expect(ACTIONS.toggleRead.label(unread)).toBe("markRead");
    expect(ACTIONS.toggleRead.label(read)).toBe("markUnread");
    expect(ACTIONS.toggleFlag.label(unread)).toBe("flag");
    expect(ACTIONS.toggleFlag.label(read)).toBe("unflag");
  });

  it("knows which folder an action needs and which rows leave the view", () => {
    const inbox = makeMailbox({ id: 1, path: "INBOX", role: "inbox" });
    const junk = makeMailbox({ id: 8, path: "Junk", role: "junk" });
    expect(requiredRole({ type: "archive" })).toBe("archive");
    expect(requiredRole({ type: "delete" })).toBe("trash");
    expect(requiredRole({ type: "spam" })).toBe("junk");
    expect(requiredRole({ type: "notSpam" })).toBe("inbox");
    expect(requiredRole({ type: "flag" })).toBeNull();
    expect(leavesFolder({ type: "archive" }, inbox)).toBe(true);
    expect(leavesFolder({ type: "move", target: 5 }, inbox)).toBe(true);
    expect(leavesFolder({ type: "move", target: 1 }, inbox)).toBe(false);
    expect(leavesFolder({ type: "markRead" }, inbox)).toBe(false);
    expect(leavesFolder({ type: "notSpam" }, junk)).toBe(true);
    expect(leavesFolder({ type: "notSpam" }, inbox)).toBe(false);
  });
});

describe("shortcuts", () => {
  it("match the character, so they follow the keyboard layout", () => {
    // "#" needs Shift on a US keyboard and not on a German one: both count.
    expect(matchesShortcut(key("#", { shiftKey: true }), { key: "#" }, "macos")).toBe(true);
    expect(matchesShortcut(key("#"), { key: "#" }, "windows")).toBe(true);
    expect(matchesShortcut(key("!", { shiftKey: true }), { key: "!" }, "windows")).toBe(true);
    // Letters: Caps Lock does not matter, Shift does.
    expect(matchesShortcut(key("E"), { key: "e" }, "macos")).toBe(true);
    expect(matchesShortcut(key("E", { shiftKey: true }), { key: "e" }, "macos")).toBe(false);
    expect(matchesShortcut(key("I", { shiftKey: true }), { key: "I", shift: true }, "macos")).toBe(true);
    expect(matchesShortcut(key("i"), { key: "I", shift: true }, "macos")).toBe(false);
  });

  it("use Command on macOS and Control on Windows, and never fire with Alt or the other modifier", () => {
    expect(matchesShortcut(key("a", { metaKey: true }), { key: "a", mod: true }, "macos")).toBe(true);
    expect(matchesShortcut(key("a", { ctrlKey: true }), { key: "a", mod: true }, "macos")).toBe(false);
    expect(matchesShortcut(key("a", { ctrlKey: true }), { key: "a", mod: true }, "windows")).toBe(true);
    expect(matchesShortcut(key("a", { metaKey: true }), { key: "a", mod: true }, "windows")).toBe(false);
    expect(matchesShortcut(key("e", { metaKey: true }), { key: "e" }, "macos")).toBe(false);
    // AltGr on Windows reports Control and Alt.
    expect(matchesShortcut(key("#", { ctrlKey: true, altKey: true }), { key: "#" }, "windows")).toBe(false);
  });

  it("are written the way each platform writes them", () => {
    expect(formatShortcut({ key: "a", mod: true }, "macos")).toBe("⌘A");
    expect(formatShortcut({ key: "a", mod: true }, "windows")).toBe("Ctrl+A");
    expect(formatShortcut({ key: "I", shift: true }, "macos")).toBe("⇧I");
    expect(formatShortcut({ key: "I", shift: true }, "windows")).toBe("Shift+I");
    expect(formatShortcut({ key: "Delete" }, "windows")).toBe("Del");
    expect(ariaShortcut({ key: "z", mod: true }, "macos")).toBe("Meta+Z");
    expect(ariaShortcut({ key: "e" }, "windows")).toBe("E");
  });

  it("stay out of text fields", () => {
    const input = document.createElement("input");
    const checkbox = Object.assign(document.createElement("input"), { type: "checkbox" });
    const area = document.createElement("textarea");
    const button = document.createElement("button");
    expect(isEditableTarget(input)).toBe(true);
    expect(isEditableTarget(area)).toBe(true);
    expect(isEditableTarget(checkbox)).toBe(false);
    expect(isEditableTarget(button)).toBe(false);
  });
});

describe("tags", () => {
  it("accepts what IMAP allows in a keyword and nothing else", () => {
    for (const ok of ["work", "Projekt-2026", "a_b", "$label1", "x".repeat(128)]) expect(isValidKeyword(ok), ok).toBe(true);
    for (const bad of ["", "two words", "(x)", "a{b", "a]b", "50%", "a*", 'say"', "back\\slash", "ümlaut", "\\Seen", "x".repeat(129), "tab\t"]) {
      expect(isValidKeyword(bad), bad).toBe(false);
    }
  });

  it("shows the user's tags, not the flags other programs keep", () => {
    expect(isUserTag("work")).toBe(true);
    expect(isUserTag("$Forwarded")).toBe(false);
    expect(isUserTag("NonJunk")).toBe(false);
    const items = [makeMessage({ keywords: ["work", "$MDNSent"] }), makeMessage({ keywords: ["Work", "urgent", "Junk"] })];
    expect(tagsOf(items)).toEqual(["urgent", "work"]);
  });
});

describe("new folders", () => {
  it("go next to the others, or below the inbox where the server keeps them all", () => {
    const flat = [
      makeMailbox({ id: 1, path: "INBOX", role: "inbox", delimiter: "/" }),
      makeMailbox({ id: 2, path: "Sent", role: "sent" }),
    ];
    expect(newFolderPath(flat, "Archive")).toBe("Archive");
    const nested = [
      makeMailbox({ id: 1, path: "INBOX", role: "inbox", delimiter: "." }),
      makeMailbox({ id: 2, path: "INBOX.Sent", role: "sent", delimiter: "." }),
      makeMailbox({ id: 3, path: "INBOX.Trash", role: "trash", delimiter: "." }),
    ];
    expect(newFolderPath(nested, "Archive")).toBe("INBOX.Archive");
  });
});
