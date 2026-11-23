import { describe, expect, it, vi } from "vitest";
import { matchesShortcut } from "../src/features/actions/shortcuts";
import {
  checkShortcut,
  COMMAND_IDS,
  defaultShortcuts,
  findConflicts,
  parseOverrides,
  sameShortcut,
  shortcutFromEvent,
} from "../src/features/keyboard/bindings";
import { arrangeEntries } from "../src/features/palette/arrange";
import type { PaletteEntry } from "../src/features/palette/entries";
import { foldText, fuzzyScore } from "../src/features/palette/fuzzy";
import { boundShortcuts, useShortcutStore } from "../src/stores/shortcuts";
import { mockCore } from "./support/core";
import { Archive } from "lucide-react";

const key = (k: string, mods: Partial<Record<"shiftKey" | "metaKey" | "ctrlKey" | "altKey", boolean>> = {}) => ({
  key: k,
  shiftKey: false,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("shortcut assignment", () => {
  it.each(["macos", "windows"] as const)("the standard assignment has no conflicts and no reserved keys (%s)", (platform) => {
    expect(findConflicts(platform, {})).toEqual([]);
    for (const id of COMMAND_IDS) {
      for (const shortcut of defaultShortcuts(id, platform)) {
        const problem = checkShortcut(id, shortcut, platform, { [id]: [] });
        expect(problem === null || problem.kind === "conflict" ? null : problem, `${id} ${shortcut.key}`).toBeNull();
      }
    }
  });

  it("refuses a key another command has, and names that command", () => {
    expect(checkShortcut("archive", { key: "r" }, "macos", {})).toEqual({ kind: "conflict", other: "reply" });
    // Letters ignore case but not Shift: Shift+R is free, "R" without Shift is "r".
    expect(checkShortcut("archive", { key: "R", shift: true }, "macos", {})).toBeNull();
    expect(checkShortcut("archive", { key: "R" }, "macos", {})).toEqual({ kind: "conflict", other: "reply" });
    // The user's own assignment counts, not the standard one.
    expect(checkShortcut("archive", { key: "r" }, "macos", { reply: [{ key: "y" }] })).toBeNull();
    expect(checkShortcut("archive", { key: "y" }, "macos", { reply: [{ key: "y" }] })).toEqual({ kind: "conflict", other: "reply" });
  });

  it("lets commands that are never available together share a key", () => {
    expect(checkShortcut("deletePermanently", { key: "d" }, "macos", { delete: [{ key: "d" }] })).toBeNull();
    expect(checkShortcut("notSpam", { key: "j", shift: true }, "macos", { spam: [{ key: "J", shift: true }] })).toBeNull();
    expect(checkShortcut("archive", { key: "#" }, "macos", {})).toEqual({ kind: "conflict", other: "delete" });
  });

  it("refuses system keys, the keys for moving around and a key the command has already", () => {
    expect(checkShortcut("archive", { key: "q", mod: true }, "macos", {})).toEqual({ kind: "system" });
    expect(checkShortcut("archive", { key: "c", mod: true }, "windows", {})).toEqual({ kind: "system" });
    expect(checkShortcut("archive", { key: "Enter" }, "macos", {})).toEqual({ kind: "fixed" });
    expect(checkShortcut("archive", { key: "j" }, "macos", {})).toEqual({ kind: "fixed" });
    expect(checkShortcut("archive", { key: "ArrowDown" }, "windows", {})).toEqual({ kind: "fixed" });
    expect(checkShortcut("archive", { key: "e" }, "macos", {})).toEqual({ kind: "duplicate" });
    // Cmd+Q is the system's on macOS only.
    expect(checkShortcut("archive", { key: "q", mod: true }, "windows", {})).toBeNull();
  });

  it("records key presses by the character they produce, like matching does", () => {
    expect(shortcutFromEvent(key("e"), "macos")).toEqual({ kind: "shortcut", shortcut: { key: "e" } });
    expect(shortcutFromEvent(key("I", { shiftKey: true }), "macos")).toEqual({ kind: "shortcut", shortcut: { key: "I", shift: true } });
    // Caps Lock: an upper-case letter without Shift is the plain letter.
    expect(shortcutFromEvent(key("E"), "macos")).toEqual({ kind: "shortcut", shortcut: { key: "e" } });
    // "#" needs Shift on a US keyboard and not on a German one: Shift is not part of it.
    expect(shortcutFromEvent(key("#", { shiftKey: true }), "windows")).toEqual({ kind: "shortcut", shortcut: { key: "#" } });
    expect(shortcutFromEvent(key("k", { metaKey: true }), "macos")).toEqual({ kind: "shortcut", shortcut: { key: "k", mod: true } });
    expect(shortcutFromEvent(key("k", { ctrlKey: true }), "windows")).toEqual({ kind: "shortcut", shortcut: { key: "k", mod: true } });
    expect(shortcutFromEvent(key("Delete", { shiftKey: true }), "windows")).toEqual({
      kind: "shortcut",
      shortcut: { key: "Delete", shift: true },
    });
    expect(shortcutFromEvent(key("Shift", { shiftKey: true }), "macos")).toEqual({ kind: "pending" });
    expect(shortcutFromEvent(key("@", { altKey: true }), "macos")).toEqual({ kind: "alt" });
    expect(shortcutFromEvent(key("k", { ctrlKey: true }), "macos")).toEqual({ kind: "otherModifier" });
    expect(shortcutFromEvent(key("Dead"), "macos")).toEqual({ kind: "unusable" });
    // What is recorded matches the same key press again.
    for (const [event, platform] of [
      [key("I", { shiftKey: true }), "macos"],
      [key("#", { shiftKey: true }), "windows"],
      [key("k", { metaKey: true }), "macos"],
    ] as const) {
      const recorded = shortcutFromEvent(event, platform);
      if (recorded.kind !== "shortcut") throw new Error("not recorded");
      expect(matchesShortcut(event, recorded.shortcut, platform)).toBe(true);
    }
  });

  it("compares shortcuts the way they match", () => {
    expect(sameShortcut({ key: "e" }, { key: "E" })).toBe(true);
    expect(sameShortcut({ key: "e" }, { key: "E", shift: true })).toBe(false);
    expect(sameShortcut({ key: "#" }, { key: "#", shift: true })).toBe(true);
    expect(sameShortcut({ key: "k", mod: true }, { key: "k" })).toBe(false);
  });

  it("reads stored data defensively", () => {
    expect(parseOverrides(null)).toEqual({});
    expect(parseOverrides([])).toEqual({});
    expect(
      parseOverrides({
        archive: [{ key: "y" }, { key: 3 }, null, { key: "" }, { key: "k", mod: true, shift: "yes" }],
        unknownCommand: [{ key: "x" }],
        reply: "r",
      }),
    ).toEqual({ archive: [{ key: "y" }, { key: "k", mod: true }] });
  });

  it("a change in the store applies to the next key press and is saved in the core", async () => {
    const core = mockCore();
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "e" }]);
    expect(useShortcutStore.getState().addShortcut("macos", "archive", { key: "y" })).toBeNull();
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "e" }, { key: "y" }]);
    // Windows keeps its own assignment.
    expect(boundShortcuts("archive", "windows")).toEqual([{ key: "e" }]);
    await vi.waitFor(() => {
      expect(core.ui.preferences.shortcuts.macos).toEqual([
        { command: "archive", shortcuts: [{ key: "e", shift: false, mod: false }, { key: "y", shift: false, mod: false }] },
      ]);
    });
    useShortcutStore.getState().removeShortcut("macos", "archive", { key: "E" });
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "y" }]);
    expect(useShortcutStore.getState().addShortcut("macos", "reply", { key: "y" })).toEqual({ kind: "conflict", other: "archive" });
    useShortcutStore.getState().resetCommand("macos", "archive");
    expect(boundShortcuts("archive", "macos")).toEqual([{ key: "e" }]);
    await vi.waitFor(() => {
      expect(core.ui.preferences.shortcuts.macos).toEqual([]);
    });
  });
});

describe("fuzzy matching", () => {
  it("finds words by their letters in order, ignoring case and accents", () => {
    expect(foldText("Löschen")).toBe("loschen");
    expect(fuzzyScore("arch", "Archive")).not.toBeNull();
    expect(fuzzyScore("ARCH", "Archive")).not.toBeNull();
    expect(fuzzyScore("loschen", "Endgültig löschen")).not.toBeNull();
    expect(fuzzyScore("mrk rd", "Mark as read")).not.toBeNull();
    expect(fuzzyScore("xyz", "Archive")).toBeNull();
    // Every word must match.
    expect(fuzzyScore("archive zzz", "Archive")).toBeNull();
  });

  it("ranks starts of words and runs of letters above scattered hits", () => {
    const word = fuzzyScore("rep", "Reply") ?? 0;
    const scattered = fuzzyScore("rep", "Remove flag permanently") ?? 0;
    expect(word).toBeGreaterThan(scattered);
    const start = fuzzyScore("set", "Settings: General") ?? 0;
    const middle = fuzzyScore("set", "Reset all") ?? 0;
    expect(start).toBeGreaterThan(middle);
  });

  it("matches extra words (group, folder path) but weighs the label more", () => {
    expect(fuzzyScore("2026", "Open folder 2026")).not.toBeNull();
    expect(fuzzyScore("projekte", "Open folder 2026", ["Projekte/2026"])).not.toBeNull();
    const inLabel = fuzzyScore("inbox", "Go to Inbox") ?? 0;
    const inExtra = fuzzyScore("inbox", "Archive", ["Inbox"]) ?? 0;
    expect(inLabel).toBeGreaterThan(inExtra);
  });
});

describe("palette order", () => {
  const entry = (key: string, label: string, group: PaletteEntry["group"], disabled?: string): PaletteEntry => ({
    key,
    label,
    group,
    icon: Archive,
    run: () => undefined,
    ...(disabled && { disabled }),
  });
  const entries = [
    entry("action:archive", "Archive", "actions", "Select a message first"),
    entry("action:compose", "New message", "actions"),
    entry("view:archive", "Go to Archive", "views"),
    entry("settings:general", "Settings: General", "settings"),
  ];
  const group = (g: string) => g;

  it("puts recently used commands on top, then the groups, runnable ones first", () => {
    const sections = arrangeEntries(entries, "", ["settings:general", "action:archive"], group);
    // A recent command that cannot run here stays in its group.
    expect(sections.map((s) => [s.id, s.entries.map((e) => e.key)])).toEqual([
      ["recent", ["settings:general"]],
      ["actions", ["action:compose", "action:archive"]],
      ["views", ["view:archive"]],
    ]);
  });

  it("with a query shows the matches, runnable ones first, and the hand-over to the search last", () => {
    const sections = arrangeEntries(
      [...entries, entry("search:query", "Search mail for “arch”", "search")],
      "arch",
      [],
      group,
    );
    expect(sections.map((s) => [s.id, s.entries.map((e) => e.key)])).toEqual([
      ["results", ["view:archive", "action:archive"]],
      ["search", ["search:query"]],
    ]);
  });
});
