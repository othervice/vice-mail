import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach } from "vitest";
import { useComposerStore } from "../src/stores/composer";
import { useDragStore } from "../src/stores/drag";
import { useMailtoStore } from "../src/stores/mailto";
import { useNoticeStore } from "../src/stores/notices";
import { useSearchStore } from "../src/stores/search";
import { useUiStore } from "../src/stores/ui";
import { useActionTargetStore } from "../src/stores/actionTarget";
import { useAiPanelStore } from "../src/stores/aiPanel";
import { useAiRequestStore } from "../src/stores/aiRequests";
import { useLayoutStore } from "../src/stores/layout";
import { usePaletteStore } from "../src/stores/palette";
import { useShortcutStore } from "../src/stores/shortcuts";
import { installSystemFakes, resetSystem } from "./support/system";

beforeEach(() => {
  installSystemFakes();
});

afterEach(() => {
  // Unmount first: components unsubscribe from core events while the IPC mock still exists.
  cleanup();
  clearMocks();
  resetSystem();
  useUiStore.setState(useUiStore.getInitialState(), true);
  useNoticeStore.setState(useNoticeStore.getInitialState(), true);
  useDragStore.setState(useDragStore.getInitialState(), true);
  useSearchStore.setState(useSearchStore.getInitialState(), true);
  useComposerStore.setState(useComposerStore.getInitialState(), true);
  // Stage 9: mailto: links waiting for the composer.
  useMailtoStore.setState(useMailtoStore.getInitialState(), true);
  // Stage 8: shortcuts, layout and recent palette commands are stored locally (fallback).
  window.localStorage.clear();
  useShortcutStore.setState(useShortcutStore.getInitialState(), true);
  useLayoutStore.setState(useLayoutStore.getInitialState(), true);
  usePaletteStore.setState(usePaletteStore.getInitialState(), true);
  useActionTargetStore.setState(useActionTargetStore.getInitialState(), true);
  // Stage 13: the AI panel (open by default) and the progress of AI requests.
  useAiPanelStore.setState({ ...useAiPanelStore.getInitialState(), open: true }, true);
  useAiRequestStore.setState(useAiRequestStore.getInitialState(), true);
  delete document.documentElement.dataset["density"];
  delete document.documentElement.dataset["textSize"];
  const root = document.documentElement;
  delete root.dataset["theme"];
  delete root.dataset["platform"];
  root.lang = "";
});
