import { render } from "@testing-library/react";
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
