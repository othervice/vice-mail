import { within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/**
 * The reading toolbar shows its primary actions (respond, file away, mark); every other action is
 * in its "More actions" menu. Returns the visible button, or opens the menu and returns the item.
 */
export async function toolbarAction(user: UserEvent, toolbar: HTMLElement, name: string | RegExp): Promise<HTMLElement> {
  const shown = within(toolbar).queryByRole("button", { name });
  if (shown) return shown;
  await user.click(within(toolbar).getByRole("button", { name: "More actions" }));
  return within(toolbar).getByRole("menuitem", { name });
}

/** The message card folds recipients and account into one line; this unfolds the full list. */
export async function showHeaderDetails(user: UserEvent, scope: HTMLElement = document.body): Promise<HTMLElement> {
  const toggle = await within(scope).findByTestId("reading-recipients");
  if (toggle.getAttribute("aria-expanded") !== "true") await user.click(toggle);
  return within(scope).getByTestId("reading-details");
}

