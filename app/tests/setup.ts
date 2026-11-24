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
import { resetListSelectForTests } from "../src/stores/listSelect";
import { usePaletteStore } from "../src/stores/palette";
import { useShortcutStore } from "../src/stores/shortcuts";
import { installSystemFakes, resetSystem } from "./support/system";
import { resetUiPreferencesForTests } from "../src/features/keyboard/uiPreferences";
import { resetMigrationForTests } from "../src/features/keyboard/useUiPreferencesSync";
import { useQuickFilterStore } from "../src/features/views/quickFilter";
import { useQuickFilterFocus } from "../src/features/views/focus";
import { useAccentStore } from "../src/features/preferences/accent";
import { resetSaveAllRequestForTests } from "../src/features/reading/saveAllRequest";
import { useBlockedFocusStore } from "../src/features/blocked/focus";
import { useConversationStore } from "../src/features/threads/preference";
import { useColumnViewsStore } from "../src/features/layout/columnViews";

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
  // Stage 8: shortcuts, layout and recent palette commands come from the core (decision 0025);
  // the web view's storage only holds old values to migrate and per-device UI flags.
  window.localStorage.clear();
  resetUiPreferencesForTests();
  resetMigrationForTests();
  window.history.replaceState(null, "", "/");
  useShortcutStore.setState(useShortcutStore.getInitialState(), true);
  useLayoutStore.setState(useLayoutStore.getInitialState(), true);
  usePaletteStore.setState(usePaletteStore.getInitialState(), true);
  useActionTargetStore.setState(useActionTargetStore.getInitialState(), true);
  // Stage 13: the AI panel (open by default) and the progress of AI requests.
  useAiPanelStore.setState({ ...useAiPanelStore.getInitialState(), open: true }, true);
  useAiRequestStore.setState(useAiRequestStore.getInitialState(), true);
  // Stage 9: lists that select messages by id, and a request waiting for one (decision 0028).
  resetListSelectForTests();
  // Stage 16: "Unread only" is a filter of this session.
  useQuickFilterStore.setState(useQuickFilterStore.getInitialState(), true);
  useQuickFilterFocus.setState(useQuickFilterFocus.getInitialState(), true);
  // Stage 16 (decision 0036): the accent from the core, "Save all" requests of the palette and
  // the way from the inbox's note to the blocked senders.
  useAccentStore.setState(useAccentStore.getInitialState(), true);
  resetSaveAllRequestForTests();
  useBlockedFocusStore.setState(useBlockedFocusStore.getInitialState(), true);
  // Decision 0039: the conversation view, a UI preference of the core.
  useConversationStore.setState(useConversationStore.getInitialState(), true);
  // Stage 17: the column's views, a UI preference of the core, and its editor.
  useColumnViewsStore.setState(useColumnViewsStore.getInitialState(), true);
  delete document.documentElement.dataset["accent"];
  delete document.documentElement.dataset["density"];
  delete document.documentElement.dataset["textSize"];
  const root = document.documentElement;
  delete root.dataset["theme"];
  delete root.dataset["platform"];
  root.lang = "";
});
