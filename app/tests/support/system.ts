import { vi } from "vitest";

// jsdom implements neither `matchMedia` nor a changeable system language. These fakes let tests
// play the operating system: light or dark appearance, preferred languages.

const DARK_QUERY = "(prefers-color-scheme: dark)";
type Listener = (event: MediaQueryListEvent) => void;

let systemDark = false;
const listeners = new Set<Listener>();

function fakeMatchMedia(query: string): MediaQueryList {
  const isDarkQuery = query === DARK_QUERY;
  return {
    get matches() {
      return isDarkQuery && systemDark;
    },
    media: query,
    onchange: null,
    addEventListener: (_type: string, listener: Listener) => {
      if (isDarkQuery) listeners.add(listener);
    },
    removeEventListener: (_type: string, listener: Listener) => {
      listeners.delete(listener);
    },
    addListener: (listener: Listener) => {
      if (isDarkQuery) listeners.add(listener);
    },
    removeListener: (listener: Listener) => {
      listeners.delete(listener);
    },
    dispatchEvent: () => false,
  } as unknown as MediaQueryList;
}

export function installSystemFakes(): void {
  window.matchMedia = fakeMatchMedia;
}

export function resetSystem(): void {
  systemDark = false;
  listeners.clear();
}

/** The OS switches between light and dark while the app runs. */
export function setSystemDark(dark: boolean): void {
  systemDark = dark;
  for (const listener of [...listeners]) {
    listener({ matches: dark, media: DARK_QUERY } as MediaQueryListEvent);
  }
}

/** Preferred languages of the OS (`navigator.languages`), restored after each test. */
export function setSystemLanguages(languages: readonly string[]): void {
  vi.spyOn(navigator, "languages", "get").mockReturnValue(languages);
}
