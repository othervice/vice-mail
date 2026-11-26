import "./styles/app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import { installFileDrop } from "./components/fileDrop";
import { PopupApp } from "./features/popup/PopupApp";
import { applyTheme, resolveTheme, systemPrefersDark } from "./features/preferences/theme";
import { createI18n, resolveLanguage } from "./i18n";

// Stage 18 (decision 0051): the new-mail popup's window. A page of its own with only what the
// popup needs; until the core answers it follows the system's theme and language.
applyTheme(resolveTheme("system", systemPrefersDark()));
const language = resolveLanguage("system", navigator.languages);
document.documentElement.lang = language;
const i18n = createI18n(language);
document.title = i18n.t("popup.title");
i18n.on("languageChanged", () => {
  document.title = i18n.t("popup.title");
});

// Stage 23 (decision 0082): a file dropped on the popup does nothing.
installFileDrop();

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing in popup.html");

createRoot(root).render(
  <StrictMode>
    <I18nextProvider i18n={i18n}>
      <PopupApp />
    </I18nextProvider>
  </StrictMode>,
);
