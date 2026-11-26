import "./styles/app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { createQueryClient } from "./app/queryClient";
import { installFileDrop } from "./components/fileDrop";
import { installDragReset } from "./stores/drag";
import { PRODUCT_NAME } from "./config/product";
import { applyTheme, resolveTheme, systemPrefersDark } from "./features/preferences/theme";
import { createI18n, resolveLanguage } from "./i18n";

// Until the stored preferences arrive from the core, follow the system, so the first frame
// already has the right colours and language.
document.title = PRODUCT_NAME;
applyTheme(resolveTheme("system", systemPrefersDark()));
const language = resolveLanguage("system", navigator.languages);
document.documentElement.lang = language;

// Stage 23 (decision 0082): files dragged onto the window are the page's to take or ignore, and
// every drag inside the page starts clean.
installFileDrop();
installDragReset();

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing in index.html");

createRoot(root).render(
  <StrictMode>
    <App queryClient={createQueryClient()} i18n={createI18n(language)} />
  </StrictMode>,
);
