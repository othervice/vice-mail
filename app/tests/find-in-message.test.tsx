// Stage 16: find in the open message. The search walks the shown text (the app's nodes and the
// sandboxed mail frame's document, read from the app; nothing runs in the frame), highlights
// through the CSS Custom Highlight API of the document the match lies in, and falls back to
// selecting the current match where the API is missing. Cmd/Ctrl+F finds in the pane and stays
// the mail search everywhere else.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearHighlights,
  collectSegments,
  FIND_CURRENT_HIGHLIGHT,
  FIND_HIGHLIGHT,
  findMatches,
  paintMatches,
} from "../src/features/reading/find";
import { useSearchStore } from "../src/stores/search";
import { htmlBody, makeAccount, makeMessage, makeView, mockCore, standardMailboxes, textBody } from "./support/core";
import type { MessageBody } from "../src/services/ipc";
import { focusRingReport } from "./support/focusRing";
import { renderApp } from "./support/render";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});

afterEach(() => {
  document.body.innerHTML = "";
  document.documentElement.style.removeProperty("--ui-mail-find-match");
  document.documentElement.style.removeProperty("--ui-mail-find-current");
});

function mount(html: string): HTMLElement {
  const root = document.createElement("div");
  root.innerHTML = html;
  document.body.appendChild(root);
  return root;
}

/** A same-origin frame with this body, as the sandboxed mail frame is for the app. */
function frameWith(root: HTMLElement, html: string): HTMLIFrameElement {
  const frame = document.createElement("iframe");
  root.appendChild(frame);
  const doc = frame.contentDocument;
  if (!doc) throw new Error("no frame document");
  doc.body.innerHTML = html;
  return frame;
}

/** A stand-in for the CSS Custom Highlight API of one window (jsdom has none). */
function installHighlights(view: Window) {
  const registry = new Map<string, { ranges: Range[] }>();
  class Highlight {
    ranges: Range[];
    constructor(...ranges: Range[]) {
      this.ranges = ranges;
    }
  }
  Object.assign(view, { CSS: { highlights: registry }, Highlight });
  return registry;
}

describe("finding text", () => {
  it("finds matches across inline elements, ignoring case, in document order", () => {
    const root = mount("<p>Budget <b>Rev</b>iew</p><p>budget review again</p>");
    const matches = findMatches(collectSegments(root), "budget review");
    expect(matches.map((match) => match.range.toString())).toEqual(["Budget Review", "budget review"]);
  });

  it("never runs from one paragraph into the next", () => {
    const root = mount("<div>end of one</div><div>start of two</div>");
    expect(findMatches(collectSegments(root), "onestart")).toHaveLength(0);
    expect(findMatches(collectSegments(root), "one")).toHaveLength(1);
  });

  it("skips what nobody sees: hidden, screen-reader only and the app's own controls", () => {
    const root = mount(
      '<p>visible word</p><p hidden>word</p><span class="sr-only">word</span><div data-find-ignore>word</div><style>.word{}</style>',
    );
    expect(findMatches(collectSegments(root), "word")).toHaveLength(1);
  });

  it("an empty or blank query finds nothing", () => {
    const root = mount("<p>anything</p>");
    expect(findMatches(collectSegments(root), "")).toEqual([]);
    expect(findMatches(collectSegments(root), "   ")).toEqual([]);
  });

  it("searches the mail frame's document as a segment of its own, between the app's text", () => {
    const root = mount("<h2>Offer before</h2>");
    const frame = frameWith(root, "<p>The offer inside the mail</p>");
    root.insertAdjacentHTML("beforeend", "<p>offer after</p>");
    const matches = findMatches(collectSegments(root), "offer");
    expect(matches.map((match) => match.segment.frame)).toEqual([null, frame, null]);
    expect(matches[1]?.range.startContainer.ownerDocument).toBe(frame.contentDocument);
  });
});

describe("highlighting", () => {
  it("registers the matches with the frame's own highlight registry and its colours as a style, never a script", () => {
    document.documentElement.style.setProperty("--ui-mail-find-match", "#fbe7a6");
    document.documentElement.style.setProperty("--ui-mail-find-current", "#f5b83d");
    const root = mount("<p>note</p>");
    const frame = frameWith(root, "<p>note one, note two</p>");
    const frameDoc = frame.contentDocument;
    const frameWindow = frame.contentWindow;
    if (!frameDoc || !frameWindow) throw new Error("no frame");
    const appRegistry = installHighlights(window);
    const frameRegistry = installHighlights(frameWindow);
    const docs = new Set<Document>();

    const matches = findMatches(collectSegments(root), "note");
    expect(matches).toHaveLength(3);
    paintMatches(matches, 1, docs);

    expect(appRegistry.get(FIND_HIGHLIGHT)?.ranges).toHaveLength(1);
    expect(appRegistry.get(FIND_CURRENT_HIGHLIGHT)?.ranges).toHaveLength(0);
    expect(frameRegistry.get(FIND_HIGHLIGHT)?.ranges).toHaveLength(1);
    expect(frameRegistry.get(FIND_CURRENT_HIGHLIGHT)?.ranges.map(String)).toEqual(["note"]);
    const style = frameDoc.querySelector("style[data-vice-find]");
    expect(style?.textContent).toContain("::highlight(vice-find-current)");
    expect(style?.textContent).toContain("#f5b83d");
    expect(frameDoc.querySelectorAll("script")).toHaveLength(0);
    // The app's own document needs no style element: app.css has the rules.
    expect(document.querySelector("style[data-vice-find]")).toBeNull();

    clearHighlights(docs);
    expect(frameRegistry.size).toBe(0);
    expect(appRegistry.size).toBe(0);
    expect(frameDoc.querySelector("style[data-vice-find]")).toBeNull();
    Reflect.deleteProperty(window, "CSS");
    Reflect.deleteProperty(window, "Highlight");
  });

  it("without the highlight API selects the current match in its frame", () => {
    const root = mount("<p>intro</p>");
    const frame = frameWith(root, "<p>alpha beta alpha</p>");
    const matches = findMatches(collectSegments(root), "alpha");
    paintMatches(matches, 1, new Set());
    const selection = frame.contentWindow?.getSelection();
    expect(selection?.toString()).toBe("alpha");
    expect(selection?.getRangeAt(0).startOffset).toBe(11);
  });
});

// ---- In the app ----

const ADA = makeAccount({ writesEnabled: true });

const TEXT = textBody([{ kind: "text", text: "The budget for 2027.\nPlease review the budget by Friday." }]);

function setup(body: MessageBody = TEXT) {
  const message = makeMessage({ id: 7001, subject: "Budget 2027", from: { name: "Grace Hopper", email: "grace@example.org" } });
  const core = mockCore({
    accounts: [ADA],
    mailboxes: standardMailboxes(),
    messages: { 1: [message] },
    views: { [message.id]: makeView(message, { body, formats: [body.kind === "html" ? "html" : "text"] }) },
    readingPreferences: { markReadOnOpen: "never" },
  });
  return { core, message, ...renderApp() };
}

async function openMessage(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("option"));
  await user.keyboard("{Enter}");
  const pane = await screen.findByRole("region", { name: "Message" });
  await within(pane).findByTestId("message-card");
  return pane;
}

describe("find in the open message", () => {
  it("Cmd+F in the reading pane opens the find bar, counts, steps and closes with Escape", async () => {
    const { user } = setup();
    const pane = await openMessage(user);
    const article = pane.querySelector("article");
    expect(document.activeElement).toBe(article);

    await user.keyboard("{Meta>}f{/Meta}");
    const input = await within(pane).findByRole("textbox", { name: "Find in this message" });
    expect(document.activeElement).toBe(input);
    // The mail search did not open.
    expect(useSearchStore.getState().open).toBe(false);

    await user.type(input, "BUDGET");
    const count = within(pane).getByTestId("find-count");
    await waitFor(() => {
      expect(count.textContent).toBe("1 of 3");
    });
    await user.keyboard("{Enter}");
    expect(count.textContent).toBe("2 of 3");
    await user.keyboard("{Enter}{Enter}");
    expect(count.textContent).toBe("1 of 3");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    expect(count.textContent).toBe("3 of 3");
    await user.click(within(pane).getByRole("button", { name: "Previous match" }));
    expect(count.textContent).toBe("2 of 3");
    await user.click(within(pane).getByRole("button", { name: "Next match" }));
    expect(count.textContent).toBe("3 of 3");

    await user.clear(input);
    await user.type(input, "nothing like this");
    await waitFor(() => {
      expect(count.textContent).toBe("No matches");
    });
    expect(within(pane).getByRole<HTMLButtonElement>("button", { name: "Next match" }).disabled).toBe(true);

    input.focus();
    await user.keyboard("{Escape}");
    expect(within(pane).queryByTestId("find-bar")).toBeNull();
    // Escape closed the bar only: the message stays open and the focus is back on it.
    expect(screen.getByRole("region", { name: "Message" })).toBeTruthy();
    expect(document.activeElement).toBe(pane.querySelector("article"));
  });

  it("Cmd+F again while the bar is open takes the focus back into its field", async () => {
    const { user } = setup();
    const pane = await openMessage(user);
    await user.keyboard("{Meta>}f{/Meta}");
    const input = await within(pane).findByTestId("find-input");
    await user.type(input, "review");
    within(pane).getByTestId("find-close").focus();
    await user.keyboard("{Meta>}f{/Meta}");
    expect(document.activeElement).toBe(input);
    expect((input as HTMLInputElement).value).toBe("review");
  });

  it("Cmd+F outside the reading pane stays the mail search", async () => {
    const { user } = setup();
    await openMessage(user);
    act(() => {
      screen.getByRole("listbox").focus();
    });
    await user.keyboard("{Meta>}f{/Meta}");
    expect(useSearchStore.getState().open).toBe(true);
    expect(screen.queryByTestId("find-bar")).toBeNull();
    act(() => {
      useSearchStore.getState().closeSearch();
    });
  });

  it("finds in the HTML mail's frame once its document is there", async () => {
    const { user } = setup(htmlBody("<p>Hello from the core</p>"));
    const pane = await openMessage(user);
    const frame = within(pane).getByTestId<HTMLIFrameElement>("mail-frame");
    const doc = frame.contentDocument;
    if (!doc) throw new Error("no frame document");
    // jsdom does not render `srcdoc`; the frame's document gets the mail's text by hand.
    doc.body.innerHTML = "<p>Hello from the core, hello again</p>";
    await user.keyboard("{Meta>}f{/Meta}");
    const input = await within(pane).findByTestId("find-input");
    await user.type(input, "hello");
    const count = within(pane).getByTestId("find-count");
    await waitFor(() => {
      expect(count.textContent).toBe("1 of 2");
    });
    // The frame finished loading something else: the bar searches again.
    doc.body.innerHTML = "<p>hello</p>";
    fireEvent.load(frame);
    await waitFor(() => {
      expect(count.textContent).toBe("1 of 1");
    });
    expect(doc.querySelectorAll("script")).toHaveLength(0);
  });

  it("says it in German", async () => {
    const message = makeMessage({ id: 7002, subject: "Haushalt" });
    mockCore({
      preferences: { theme: "system", language: "de" },
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message, { body: textBody([{ kind: "text", text: "Haushalt und Haushalt" }]), formats: ["text"] }) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("option"));
    await user.keyboard("{Enter}");
    const pane = await screen.findByRole("region", { name: "Nachricht" });
    await within(pane).findByTestId("message-card");
    await user.keyboard("{Meta>}f{/Meta}");
    const input = await within(pane).findByRole("textbox", { name: "In dieser Mail suchen" });
    await user.type(input, "haushalt");
    await waitFor(() => {
      // Subject and body.
      expect(within(pane).getByTestId("find-count").textContent).toBe("1 von 3");
    });
  });
});

describe.each(["light", "dark"] as const)("find bar, %s theme", (theme) => {
  it("has no serious axe findings and a focus ring on every control", async () => {
    const message = makeMessage({ id: 7003, subject: "Budget" });
    mockCore({
      preferences: { theme, language: "system" },
      accounts: [ADA],
      mailboxes: standardMailboxes(),
      messages: { 1: [message] },
      views: { [message.id]: makeView(message, { body: textBody([{ kind: "text", text: "budget budget" }]), formats: ["text"] }) },
      readingPreferences: { markReadOnOpen: "never" },
    });
    const { user } = renderApp();
    const pane = await openMessage(user);
    await waitFor(() => {
      expect(document.documentElement.dataset["theme"]).toBe(theme);
    });
    await user.keyboard("{Meta>}f{/Meta}");
    await user.type(await within(pane).findByTestId("find-input"), "budget");
    await waitFor(() => {
      // The subject above the card is shown text too.
      expect(within(pane).getByTestId("find-count").textContent).toBe("1 of 3");
    });
    const results = await axe.run(document.body, {
      resultTypes: ["violations"],
      iframes: false,
      rules: { "color-contrast": { enabled: false } },
    });
    const serious = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id);
    expect(serious).toEqual([]);
    const report = await focusRingReport(within(pane).getByTestId("find-bar"));
    expect(report.findings).toEqual([]);
    expect(report.checked).toBeGreaterThanOrEqual(4);
  });
});
