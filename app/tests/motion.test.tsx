import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import tokensCss from "../src/styles/tokens.css?raw";
import appCss from "../src/styles/app.css?raw";
import { makeAccount, makeMessages, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";
import { appStylesheet, compiledCss } from "./support/styles";

/*
 * Stage 8, "Bewegung" and "Bewegung reduzieren" (docs/DESIGN.md: 120, 180, 240 ms, soft ease-out;
 * none under "reduce motion"; movement only where it shows where something goes).
 *
 * Decision: the app follows the system setting (`prefers-reduced-motion`: macOS "Reduce motion",
 * Windows "Animation effects" off, both passed on by WKWebView and WebView2). There is no switch
 * of its own: docs/SPEC.md asks that the system setting is respected, and the UI preferences of
 * the core (`UiPreferences`) have no field for one.
 *
 * Review: the only transitions in the app are colour changes on hover and state (`transition-colors`,
 * the fast token); nothing moves, slides or spins, so there is nothing that moves without showing
 * where something goes. The test below keeps it that way: a new kind of motion has to be added
 * here on purpose.
 */

const sources = import.meta.glob<string>(["../src/**/*.{ts,tsx,css}", "!../src/generated/**"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Motion utilities the app may use. Colour fades only: they move nothing. */
const ALLOWED_MOTION = new Set(["transition-colors"]);

const MOTION_CLASS = /(?<![\w:-])(?:[\w-]+:)*(transition(?:-[\w[\]()-]+)?|animate-[\w[\]()-]+|duration-[\w[\]()-]+|delay-[\w[\]()-]+|ease-[\w[\]()-]+)(?![\w-])/g;

function tokenBlock(selector: RegExp): string {
  return selector.exec(tokensCss)?.[1] ?? "";
}

describe("motion", () => {
  it("has the durations and the soft ease-out of DESIGN.md, and none when motion is reduced", () => {
    const root = tokenBlock(/:root\s*\{([^}]*)\}/);
    expect(root).toMatch(/--ui-duration-fast:\s*120ms;/);
    expect(root).toMatch(/--ui-duration-base:\s*180ms;/);
    expect(root).toMatch(/--ui-duration-slow:\s*240ms;/);
    // Ease-out: fast start, slow end (the curve's first control point climbs steeply).
    expect(root).toMatch(/--ui-ease-out:\s*cubic-bezier\(0\.22, 1, 0\.36, 1\);/);
    const reduced = /@media \(prefers-reduced-motion: reduce\)\s*\{\s*:root\s*\{([^}]*)\}/.exec(tokensCss)?.[1] ?? "";
    for (const name of ["fast", "base", "slow"]) expect(reduced).toMatch(new RegExp(`--ui-duration-${name}:\\s*0ms;`));
    // And no transition or animation at all, whatever its duration says.
    expect(appCss).toMatch(/@media \(prefers-reduced-motion: reduce\)\s*\{\s*\*,\s*\*::before,\s*\*::after\s*\{[^}]*animation-duration: 0ms !important;[^}]*transition-duration: 0ms !important;/);
  });

  it("uses only colour fades in the components, from the tokens, without a smooth scroll or script animation", async () => {
    const found = new Map<string, Set<string>>();
    for (const [path, source] of Object.entries(sources)) {
      if (path.endsWith(".css")) {
        expect(source, path).not.toMatch(/@keyframes|animation\s*:|transition\s*:/);
        continue;
      }
      expect(source, path).not.toMatch(/behavior:\s*["']smooth["']|\.animate\(|scroll-smooth/);
      for (const match of source.matchAll(MOTION_CLASS)) {
        const name = match[1] ?? "";
        if (!found.has(name)) found.set(name, new Set());
        found.get(name)?.add(path);
      }
    }
    const notAllowed = [...found.entries()].filter(([name]) => !ALLOWED_MOTION.has(name)).map(([name, paths]) => `${name}: ${[...paths].join(", ")}`);
    expect(notAllowed).toEqual([]);
    const css = await compiledCss([...ALLOWED_MOTION]);
    expect(css).toContain("transition-duration: var(--tw-duration, var(--ui-duration-fast));");
    expect(css).toContain("transition-timing-function: var(--tw-ease, var(--ui-ease-out));");
    expect(css).toMatch(/transition-property: color, background-color, border-color, outline-color, text-decoration-color, fill, stroke/);
  });

  it("turns every transition on the page off when the system asks to reduce motion", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    const items = makeMessages(3);
    mockCore({
      accounts: [makeAccount({ writesEnabled: true })],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    });
    renderApp();
    await screen.findByRole("listbox", { name: /^Messages in/ });
    await screen.findAllByRole("option");
    const sheet = await appStylesheet();
    const moving = [...document.body.querySelectorAll("[class*='transition']")];
    expect(moving.length).toBeGreaterThan(5);
    for (const element of moving) {
      expect(sheet.resolved(element, "transition-duration")).toBe("var(--ui-duration-fast)");
      expect(sheet.value(element, "transition-duration", { reducedMotion: true })).toBe("0ms");
      expect(sheet.value(element, "animation-duration", { reducedMotion: true })).toBe("0ms");
    }
  });
});
