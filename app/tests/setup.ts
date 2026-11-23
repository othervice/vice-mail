import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach } from "vitest";
import { useDragStore } from "../src/stores/drag";
import { useNoticeStore } from "../src/stores/notices";
import { useUiStore } from "../src/stores/ui";
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
  const root = document.documentElement;
  delete root.dataset["theme"];
  delete root.dataset["platform"];
  root.lang = "";
});
