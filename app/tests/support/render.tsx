import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../src/app/App";
import { createQueryClient } from "../../src/app/queryClient";
import { createI18n, type Language } from "../../src/i18n";

/** Renders the whole app the way main.tsx does, against whatever `mockCore` set up. */
export function renderApp(initialLanguage: Language = "en") {
  const queryClient = createQueryClient();
  const i18n = createI18n(initialLanguage);
  const user = userEvent.setup();
  const view = render(<App queryClient={queryClient} i18n={i18n} />);
  return { ...view, user, queryClient, i18n };
}

export function rootTheme(): string | undefined {
  return document.documentElement.dataset["theme"];
}

/**
 * Stage 18 (task F): opening a folder shows its top message by itself. Tests that start from "no
 * message open" close it first (the pane's close button clears the selection; the list keeps its
 * size and the pane's place shows "No message selected").
 */
export async function closeOpenedMessage(user: { click: (element: Element) => Promise<void> }): Promise<void> {
  const pane = await screen.findByRole("region", { name: "Message" });
  await user.click(within(pane).getByRole("button", { name: "Close message" }));
  await screen.findByTestId("reading-empty");
}
