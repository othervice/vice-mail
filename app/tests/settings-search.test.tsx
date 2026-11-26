import { act, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import { createI18n, type Language } from "../src/i18n";
import { fold, resolveIndex, searchSettings } from "../src/features/settings/search/searchSettings";
import { FOUND_MS } from "../src/features/settings/search/jump";
import { SETTINGS_INDEX } from "../src/features/settings/search/settingsIndex";
import { SETTINGS_SECTION_IDS, useUiStore, type SettingsSectionId } from "../src/stores/ui";
import { makeAiProfile } from "./support/ai";
import { makeAccount, makeMessages, mockCore, standardMailboxes } from "./support/core";
import { makeDevice, makeMcpClient, syncStatus } from "./support/mcpSync";
import { renderApp } from "./support/render";

/*
 * Stage 22, item 8 (decision 0073): searching the settings. The index holds every option by the
 * i18n keys its row renders (no second copy of a text); the search matches titles and
 * descriptions in the UI's language, lists the hits grouped by section, and goes to the row: the
 * section opens, the row scrolls into view, its control gets the focus, the row is marked for a
 * moment. Cmd/Ctrl+F in the settings puts the focus into the field; combobox and listbox ARIA;
 * Escape clears.
 *
 * "Hard to forget": every section is rendered (in two states, in English and German) and every
 * row that marks itself as an option must be in the index and say what its entry says; every
 * control of a section must sit in such a row or in a block found by its heading.
 */

const ADA = makeAccount({ writesEnabled: true });

type Scenario = "plain" | "rich";

function core(scenario: Scenario, language: Language = "en") {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  return mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: makeMessages(2) },
    preferences: { theme: "light", language },
    // AI with a provider, MCP with a client, the sync running, the updater in this build.
    ...(scenario === "rich" && {
      ai: { enabled: true, profiles: [makeAiProfile()], openReplies: { enabled: true, days: 7 } },
      mcpSync: {
        mcp: { enabled: true, clients: [makeMcpClient()] },
        sync: { status: syncStatus({ state: "active", accountId: ADA.id, folder: "Settings-Sync", lastSyncAt: 1, devices: [makeDevice()] }) },
      },
      updates: { info: { supported: true, autoCheck: true } },
    }),
  });
}

async function openSettings(section: SettingsSectionId = "general") {
  await screen.findByRole("listbox", { name: /^(Messages in|Nachrichten in)/ });
  act(() => {
    useUiStore.getState().showSettings(section);
  });
  return screen.findByRole("combobox", { name: /^(Search settings|Einstellungen durchsuchen)$/ });
}

function panelOf(section: SettingsSectionId): HTMLElement {
  const panel = document.querySelector<HTMLElement>(`[data-settings-panel="${section}"]`);
  if (!panel) throw new Error(`no page for ${section}`);
  return panel;
}

/** Lets a section load what it asks the core for. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 200));
  });
}

const CONTROLS = 'input, textarea, select, [role="switch"], [role="radiogroup"], [role="combobox"], [role="checkbox"], [role="slider"], [role="spinbutton"]';

describe("the search itself", () => {
  const en = resolveIndex(createI18n("en").t);
  const de = resolveIndex(createI18n("de").t);
  const titles = (groups: ReturnType<typeof searchSettings>) => groups.flatMap((group) => group.hits.map((hit) => `${group.section}: ${hit.entry.title}`));

  it("folds case and accents without moving a character", () => {
    expect(fold("Überfahren STRASSE Größe")).toBe("uberfahren strasse große");
    expect(fold("Größe").length).toBe("Größe".length);
  });

  it("finds options by title and description in the current language, grouped by section", () => {
    // German: "Signatur" finds the signatures and the default signatures (Writing).
    const signatur = searchSettings(de, "Signatur");
    expect(signatur.map((group) => group.section)).toEqual(["compose"]);
    expect(titles(signatur)).toEqual(expect.arrayContaining(["compose: Signaturen", "compose: Standard-Signaturen jeder Absenderadresse."]));
    // "Ton" finds the sound of notifications, by title.
    const ton = titles(searchSettings(de, "Ton"));
    expect(ton).toEqual(expect.arrayContaining(["notifications: Ton", "notifications: Ton abspielen"]));
    // English: by a word of the description ("Undo send" says "take it back").
    expect(titles(searchSettings(en, "take it back"))).toContain("compose: Undo send");
    // Every word must occur, in any order; accents need not be typed.
    expect(titles(searchSettings(de, "dunkel nachrichten"))).toContain("reading: Nachrichten im dunklen Design");
    expect(titles(searchSettings(de, "menuleiste"))).toEqual(expect.arrayContaining(["general: Symbol in der Menüleiste zeigen"]));
    expect(searchSettings(en, "   ")).toEqual([]);
    expect(searchSettings(en, "no such option anywhere")).toEqual([]);
  });

  it("puts the strongest hits first and keeps the sections in the order of the list", () => {
    const groups = searchSettings(en, "notifications");
    expect(groups.map((group) => group.section)).toEqual(
      SETTINGS_SECTION_IDS.filter((section) => groups.some((group) => group.section === section)),
    );
    const own = groups.find((group) => group.section === "notifications");
    // The section itself, then the titles that start with the word.
    expect(own?.hits[0]?.entry.isSection).toBe(true);
    expect(own?.hits[0]?.titleRanges).toEqual([{ start: 0, end: "notifications".length }]);
    expect(own?.hits.slice(1, 3).map((hit) => hit.entry.title)).toEqual(["Notifications", "Notifications"]);
  });

  it("tells rows with the same title in one section apart by their key", () => {
    for (const index of [en, de]) {
      for (const section of SETTINGS_SECTION_IDS) {
        const rows = index.filter((entry) => entry.section === section && !entry.isSection);
        for (const entry of rows) {
          const same = rows.filter((other) => other.title === entry.title);
          if (same.length > 1) expect(same.map((other) => other.key !== null), `${section}: ${entry.title}`).not.toContain(false);
        }
      }
    }
    expect(new Set(SETTINGS_INDEX.map((entry) => `${entry.section}:${entry.title}`)).size).toBe(SETTINGS_INDEX.length);
  });
});

/** Rows of the index the tests do not bring on screen: the states they need, said here. */
const NOT_RENDERED = new Set([
  // The sync while it is off shows "Turn on settings sync"; the other states of the sync:
  "sync:settingsSync.join.title", // a sync folder found in an account
  "sync:settingsSync.needsPassphrase.title", // the passphrase changed on another device
  "sync:settingsSync.passwords.title", // accounts taken over without a password
  "sync:settingsSync.questions.title", // an account removed on another device
]);

describe("every option is in the index (decision 0073)", () => {
  const rendered = new Set<string>();
  for (const language of ["en", "de"] as const) {
    for (const scenario of ["plain", "rich"] as const) {
      it(`every row that marks itself is found and says what its entry says (${language}, ${scenario})`, async () => {
        core(scenario, language);
        const view = renderApp(language);
        await openSettings();
        for (const section of SETTINGS_SECTION_IDS) {
          act(() => {
            useUiStore.getState().showSettings(section);
          });
          await settle();
          const index = resolveIndex(view.i18n.t);
          const panel = panelOf(section);
          const problems: string[] = [];
          for (const row of panel.querySelectorAll<HTMLElement>("[data-setting-title]")) {
            const title = row.dataset["settingTitle"] ?? "";
            const key = row.dataset["settingKey"];
            const entry = index.find(
              (candidate) =>
                candidate.section === section && !candidate.isSection && candidate.title === title && (candidate.key === null || candidate.key === key),
            );
            if (!entry) {
              problems.push(`not in the index: "${title}"${key ? ` (${key})` : ""}`);
              continue;
            }
            rendered.add(entry.id);
            // The description is the row's or its block's.
            if (entry.descriptions.length > 0) {
              const around = row.closest("[data-setting-block]") ?? row;
              const text = `${row.textContent ?? ""} ${around.textContent ?? ""}`;
              if (!entry.descriptions.some((description) => text.includes(description))) problems.push(`"${title}" does not say its description`);
            }
          }
          for (const control of panel.querySelectorAll<HTMLElement>(CONTROLS)) {
            if (control.closest('[role="radiogroup"]') && control.getAttribute("role") !== "radiogroup") continue;
            if (control.closest("[data-setting-title], [data-setting-block]")) continue;
            problems.push(`a control in no option or block: ${control.outerHTML.slice(0, 120)}`);
          }
          expect(problems, `${language}, ${scenario}, ${section}`).toEqual([]);
        }
      }, 30_000);
    }
  }

  it("and every entry of the index is shown by some section", () => {
    const missing = SETTINGS_INDEX.map((entry) => `${entry.section}:${entry.title}`).filter((id) => !rendered.has(id) && !NOT_RENDERED.has(id));
    expect(missing, "entries of the index no section shows").toEqual([]);
  });
});

describe("the search field in the settings", () => {
  it("lists the hits grouped by section, moves with the arrow keys and goes to the row with Enter", async () => {
    core("plain", "de");
    const { user } = renderApp("de");
    const field = await openSettings("general");
    expect(field.getAttribute("aria-expanded")).toBe("false");
    expect(field.getAttribute("aria-autocomplete")).toBe("list");
    const list = document.getElementById(field.getAttribute("aria-controls") ?? "") as HTMLElement;
    expect(list.getAttribute("role")).toBe("listbox");
    await user.click(field);
    await user.keyboard("Signatur");
    expect(field.getAttribute("aria-expanded")).toBe("true");
    const groups = within(list).getAllByRole("group");
    expect(groups.map((group) => group.getAttribute("aria-labelledby") && document.getElementById(group.getAttribute("aria-labelledby") ?? "")?.textContent)).toEqual(["Schreiben"]);
    const options = within(list).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining("Signaturen"), expect.stringContaining("Standard-Signaturen")]),
    );
    // The found places are marked.
    expect(within(options[0] as HTMLElement).getAllByTestId("highlight")[0]?.textContent?.toLowerCase()).toBe("signatur");
    expect(screen.getByTestId("settings-search-status").textContent).toMatch(/^\d+ Einstellungen? gefunden$/);
    // The first hit is active; the arrow keys move, the focus stays in the field.
    expect(field.getAttribute("aria-activedescendant")).toBe(options[0]?.id);
    await user.keyboard("{ArrowDown}");
    expect(field.getAttribute("aria-activedescendant")).toBe(options[1]?.id);
    expect(options[1]?.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{ArrowUp}");
    expect(field.getAttribute("aria-activedescendant")).toBe(options[0]?.id);
    expect(document.activeElement).toBe(field);
    const results = await axe.run(document.body, { resultTypes: ["violations"], rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);

    // Enter: the section opens, the row is marked for a moment and its block gets the focus.
    const chosen = options[0]?.textContent ?? "";
    await user.keyboard("{Enter}");
    expect(field.getAttribute("aria-expanded")).toBe("false");
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Schreiben" }).getAttribute("aria-selected")).toBe("true");
    });
    const row = await waitFor(() => {
      const found = panelOf("compose").querySelector<HTMLElement>("[data-setting-found]");
      if (!found) throw new Error("not marked");
      return found;
    });
    expect(chosen.startsWith(row.dataset["settingTitle"] ?? "-")).toBe(true);
    expect(row.contains(document.activeElement) || document.activeElement === row).toBe(true);
    expect(row.className).toContain("data-[setting-found]:bg-accent-soft");
    expect(row.className).toContain("transition-colors");
    // The mark goes again.
    await waitFor(
      () => {
        expect(row.hasAttribute("data-setting-found")).toBe(false);
      },
      { timeout: FOUND_MS + 1000 },
    );
  });

  it("goes to a switch and gives it the focus; a click on a hit does the same", async () => {
    core("plain", "de");
    const { user } = renderApp("de");
    const field = await openSettings("about");
    await user.click(field);
    await user.keyboard("beim Antworten");
    const option = within(screen.getByTestId("settings-search-results")).getAllByRole("option")[0] as HTMLElement;
    expect(option.textContent).toContain("Ursprungsmail beim Antworten archivieren");
    await user.click(option);
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Schreiben" }).getAttribute("aria-selected")).toBe("true");
    });
    await waitFor(() => {
      expect(document.activeElement?.getAttribute("role")).toBe("switch");
    });
    expect(document.activeElement?.closest("[data-setting-title]")?.getAttribute("data-setting-title")).toBe("Ursprungsmail beim Antworten archivieren");
  });

  it("tells rows with the same title apart: the sound's choice, not its group", async () => {
    core("plain", "en");
    const { user } = renderApp("en");
    const field = await openSettings("general");
    await user.click(field);
    await user.keyboard("sound");
    const hits = within(screen.getByTestId("settings-search-results")).getAllByRole("option");
    const choice = hits.find((hit) => hit.textContent?.startsWith("Sound") && hit.textContent.includes("next notification")) as HTMLElement;
    await user.click(choice);
    const row = await waitFor(() => {
      const found = panelOf("notifications").querySelector<HTMLElement>("[data-setting-found]");
      if (!found) throw new Error("not marked");
      return found;
    });
    expect(row.dataset["settingKey"]).toBe("sounds.choice");
  });

  it("Escape clears the field, an unknown word says so, Cmd+F puts the focus into it", async () => {
    core("plain", "en");
    const { user } = renderApp("en");
    const field = await openSettings("reading");
    await user.click(field);
    await user.keyboard("zzqx");
    expect(screen.getByTestId("settings-search-status").textContent).toBe("No setting found for “zzqx”.");
    expect(screen.getByTestId("settings-search-results").hidden).toBe(true);
    await user.keyboard("{Escape}");
    expect((field as HTMLInputElement).value).toBe("");
    expect(field.getAttribute("aria-expanded")).toBe("false");
    // The page keeps the settings open (Escape did not leave them).
    expect(screen.getByRole("tab", { name: "Reading" }).getAttribute("aria-selected")).toBe("true");
    // Cmd+F from the page (the shortcut of the search, macOS here).
    act(() => {
      screen.getByRole("tab", { name: "Reading" }).focus();
    });
    await user.keyboard("{Meta>}f{/Meta}");
    expect(document.activeElement).toBe(field);
    // A section's name finds the section.
    await user.keyboard("Keyboard");
    const hit = within(screen.getByTestId("settings-search-results")).getAllByRole("option")[0] as HTMLElement;
    expect(hit.textContent).toContain("The whole page");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Keyboard" }).getAttribute("aria-selected")).toBe("true");
    });
  });

  it("speaks the UI's language when it changes", async () => {
    core("plain", "en");
    const { user } = renderApp("en");
    const field = await openSettings("general");
    await user.click(field);
    await user.keyboard("theme");
    expect(within(screen.getByTestId("settings-search-results")).getAllByRole("option").map((o) => o.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining("Theme")]),
    );
    await user.keyboard("{Escape}");
    // The language as the user sets it (General → Language).
    await user.click(screen.getByRole("radio", { name: "Deutsch" }));
    await waitFor(() => {
      expect(field.getAttribute("aria-label")).toBe("Einstellungen durchsuchen");
    });
    await user.click(field);
    await user.keyboard("Design");
    const hits = within(screen.getByTestId("settings-search-results")).getAllByRole("option");
    expect(hits.map((hit) => hit.textContent)).toEqual(expect.arrayContaining([expect.stringContaining("Design")]));
  });
});
