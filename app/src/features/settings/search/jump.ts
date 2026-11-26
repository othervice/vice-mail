import { useCallback, useEffect, useEffectEvent, useState, type RefObject } from "react";
import { boundShortcuts } from "../../../stores/shortcuts";
import { useUiStore, type SettingsSectionId } from "../../../stores/ui";
import { isEditableTarget, matchesShortcut, type ShortcutPlatform } from "../../actions/shortcuts";
import type { ResolvedEntry } from "./searchSettings";

/*
 * Stage 22, item 8 (decision 0073): going from a hit of the settings search to its row. The
 * section opens; once its row is there (a section may still load its data) it is scrolled into
 * the middle of the page, its control gets the focus (else the row itself), and the row is marked
 * for a moment (`data-setting-found`: the soft accent behind it, faded like every colour change,
 * so not at all under "reduce motion"; no smooth scrolling either). A row that the section does
 * not show in its state (a block of the sync while it is off) leaves the focus on the page.
 */

/** How long the row stays marked. */
export const FOUND_MS = 2000;

/** How long the jump waits for a section to show its row. */
const WAIT_MS = 3000;

/** Where the focus goes in a row: its chosen option, else its first control that is enabled. */
const CONTROLS = [
  '[role="radio"][aria-checked="true"]:not([disabled])',
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex="0"]',
];

/** The row of `entry` in a section's page, by its key where it has one, else by its title. */
export function findSettingRow(panel: ParentNode, entry: ResolvedEntry): HTMLElement | null {
  if (entry.key !== null) {
    for (const row of panel.querySelectorAll<HTMLElement>("[data-setting-key]")) {
      if (row.dataset["settingKey"] === entry.key) return row;
    }
  }
  for (const row of panel.querySelectorAll<HTMLElement>("[data-setting-title]")) {
    if (row.dataset["settingTitle"] === entry.title && (entry.key === null || row.dataset["settingKey"] === undefined)) return row;
  }
  return null;
}

function controlOf(row: HTMLElement): HTMLElement | undefined {
  return CONTROLS.map((selector) => row.querySelector<HTMLElement>(selector)).find((found) => found !== null);
}

/**
 * Scrolls to the row, focuses it and marks it. A row whose control is still disabled (its section
 * is loading what it shows) gets the focus itself, and its control once it is enabled, unless the
 * focus went elsewhere meanwhile.
 */
function reveal(row: HTMLElement): void {
  row.scrollIntoView?.({ block: "center" });
  const control = controlOf(row);
  if (control) {
    control.focus({ preventScroll: true });
  } else {
    if (!row.hasAttribute("tabindex")) row.tabIndex = -1;
    row.focus({ preventScroll: true });
    if (row.querySelector("[disabled]")) {
      const observer = new MutationObserver(() => {
        const enabled = controlOf(row);
        if (!enabled) return;
        observer.disconnect();
        if (document.activeElement === row) enabled.focus({ preventScroll: true });
      });
      observer.observe(row, { subtree: true, attributes: true, attributeFilter: ["disabled"] });
      window.setTimeout(() => {
        observer.disconnect();
      }, WAIT_MS);
    }
  }
  row.setAttribute("data-setting-found", "");
  window.setTimeout(() => {
    row.removeAttribute("data-setting-found");
  }, FOUND_MS);
}

/**
 * The jump of the settings view: opens the hit's section and goes to its row as soon as the
 * page shows it. `panels` holds the pages (`data-settings-panel`), `section` is the open one.
 */
export function useSettingsJump(panels: RefObject<HTMLElement | null>, section: SettingsSectionId): (entry: ResolvedEntry) => void {
  const showSettings = useUiStore((state) => state.showSettings);
  const [target, setTarget] = useState<{ entry: ResolvedEntry; serial: number } | null>(null);

  useEffect(() => {
    const container = panels.current;
    if (!target || target.entry.section !== section || !container) return;
    const panelOf = () => container.querySelector<HTMLElement>(`[data-settings-panel="${section}"]`);
    const attempt = (): boolean => {
      const panel = panelOf();
      if (!panel) return false;
      if (target.entry.isSection) {
        panel.scrollTop = 0;
        panel.focus({ preventScroll: true });
        return true;
      }
      const row = findSettingRow(panel, target.entry);
      if (!row) return false;
      reveal(row);
      return true;
    };
    if (attempt()) return;
    const observer = new MutationObserver(() => {
      if (attempt()) stop();
    });
    const timer = window.setTimeout(() => {
      stop();
      // Not shown in this state: the page itself.
      panelOf()?.focus({ preventScroll: true });
    }, WAIT_MS);
    function stop() {
      observer.disconnect();
      window.clearTimeout(timer);
    }
    observer.observe(container, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-setting-title"] });
    return stop;
  }, [panels, section, target]);

  return useCallback(
    (entry: ResolvedEntry) => {
      showSettings(entry.section);
      setTarget((previous) => ({ entry, serial: (previous?.serial ?? 0) + 1 }));
    },
    [showSettings],
  );
}

/**
 * The search shortcut in the settings (Cmd/Ctrl+F, "/", as the user bound `search`): the focus
 * into the settings search instead of the mail search, which the settings view has not.
 */
export function useSettingsSearchShortcut(platform: ShortcutPlatform, focus: () => void): void {
  const onKey = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.repeat) return;
    const match = boundShortcuts("search", platform).find((shortcut) => matchesShortcut(event, shortcut, platform));
    if (!match) return;
    // A plain key ("/") is text in a field; Cmd/Ctrl+F works from fields too.
    if (!match.mod && isEditableTarget(event.target)) return;
    event.preventDefault();
    focus();
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      onKey(event);
    };
    window.addEventListener("keydown", listener);
    return () => {
      window.removeEventListener("keydown", listener);
    };
  }, []);
}
