import { screen, within } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";

type User = ReturnType<typeof userEvent.setup>;

/**
 * Stage 23 (decision 0086): the composer keeps its options in menus (identity, paragraph style,
 * "More formatting", the send button's arrow, the reminder and "…"). These open one by the test
 * id of its button and find an entry by its role and name, as a user (or a screen reader) would.
 */

/** The entries of an open menu: commands, choices and options. */
export function menuEntries(menu: HTMLElement): HTMLElement[] {
  return [...menu.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"]')];
}

/** The entry named `name` of an open menu (exact name, or a pattern). */
export function menuEntry(menu: HTMLElement, name: string | RegExp): HTMLElement {
  const found = menuEntries(menu).find((entry) => {
    const own = entry.getAttribute("aria-label") ?? (entry.textContent ?? "").trim();
    const label = [...entry.querySelectorAll("span")].map((span) => span.textContent ?? "");
    const names = [own, ...label].map((text) => text.trim());
    return names.some((text) => (typeof name === "string" ? text === name : name.test(text)));
  });
  if (!found) throw new Error(`no menu entry ${String(name)} in ${menuEntries(menu).map((e) => e.textContent).join(" | ")}`);
  return found;
}

/** Opens the menu of the composer's button with this test id; returns the menu. */
export async function openComposerMenu(user: User, testId: string): Promise<HTMLElement> {
  const composer = screen.getByTestId("composer");
  const button = within(composer).getByTestId(testId);
  if (button.getAttribute("aria-expanded") !== "true") await user.click(button);
  return within(composer).getByTestId(`${testId}-menu`);
}

/** Opens the menu and chooses the entry named `name`. */
export async function chooseInComposerMenu(user: User, testId: string, name: string | RegExp): Promise<void> {
  const menu = await openComposerMenu(user, testId);
  await user.click(menuEntry(menu, name));
}

/** Closes an open menu with Escape (the focus goes back to its button). */
export async function closeComposerMenu(user: User): Promise<void> {
  await user.keyboard("{Escape}");
}
