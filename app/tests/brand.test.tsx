import { render, screen, within } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import brandSource from "../src/components/Brand.tsx?raw";
import { AppIcon, Wordmark } from "../src/components/Brand";
import { PRODUCT_NAME, PROJECT_URL, PUBLISHER } from "../src/config/product";
import { mockCore } from "./support/core";
import { renderApp } from "./support/render";

// Design pass 23.11.2026: the brand artwork of docs/brand, inlined, on the first start and on the
// About page.

describe("brand artwork", () => {
  it("is plain inline SVG: no style elements, no external references", () => {
    const { container } = render(
      <>
        <AppIcon />
        <Wordmark aria-label={PRODUCT_NAME} />
      </>,
    );
    expect(container.querySelectorAll("svg")).toHaveLength(2);
    expect(container.querySelector("style")).toBeNull();
    expect(container.querySelector("[style]")).toBeNull();
    for (const element of container.querySelectorAll("*")) {
      // The SVG namespace is a name, not something the app loads.
      for (const attribute of element.getAttributeNames().filter((name) => name !== "xmlns")) {
        expect(element.getAttribute(attribute) ?? "", `${element.tagName} ${attribute}`).not.toMatch(/https?:|url\((?!#)/);
      }
    }
  });

  it("gives every instance its own gradient ids, so copies never point at each other", () => {
    const { container } = render(
      <>
        <Wordmark aria-label={PRODUCT_NAME} />
        <Wordmark />
      </>,
    );
    const ids = [...container.querySelectorAll("[id]")].map((element) => element.id);
    expect(ids.length).toBeGreaterThan(0);
    expect(new Set(ids).size).toBe(ids.length);
    for (const element of container.querySelectorAll("[fill^='url(#']")) {
      const target = /url\(#([^)]+)\)/.exec(element.getAttribute("fill") ?? "")?.[1] ?? "";
      expect(element.closest("svg")?.querySelector(`[id="${target}"]`), target).not.toBeNull();
    }
  });

  // Decision 0068: the artwork is generated (docs/brand/make_svgs.py, scripts/icons.sh); the
  // inline copy must follow it, so a new colour scheme cannot leave the app behind.
  it("uses exactly the colours of docs/brand", () => {
    const colours = (text: string) => new Set([...text.matchAll(/#[0-9a-fA-F]{6}\b/g)].map(([hex]) => hex.toLowerCase()));
    const inline = colours(brandSource);
    // Outside the app's root, so read from disk rather than through Vite.
    const brandDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "docs", "brand");
    const read = (name: string) => readFileSync(join(brandDir, name), "utf8");
    const artwork = colours(read("icon.svg") + read("wordmark-dark.svg"));
    expect(inline.size).toBeGreaterThan(20);
    expect([...inline].filter((hex) => !artwork.has(hex))).toEqual([]);
    // "Vice" is `currentColor` inline; white only exists in the dark wordmark file.
    expect([...artwork].filter((hex) => !inline.has(hex) && hex !== "#ffffff")).toEqual([]);
  });

  it("is named by the caller or hidden from screen readers", () => {
    render(
      <>
        <Wordmark aria-label={PRODUCT_NAME} />
        <Wordmark lettering />
        <AppIcon />
      </>,
    );
    expect(screen.getAllByRole("img")).toHaveLength(1);
    expect(screen.getByRole("img", { name: PRODUCT_NAME })).toBeTruthy();
  });
});

describe("first start and About", () => {
  it("the first start shows the wordmark, the next step and what holds from the first account on", async () => {
    mockCore();
    renderApp();
    const welcome = await screen.findByTestId("welcome");
    expect(within(welcome).getByRole("img", { name: PRODUCT_NAME })).toBeTruthy();
    expect(within(welcome).getByRole("heading", { name: "No account yet" })).toBeTruthy();
    expect(within(welcome).getByRole("button", { name: "Add account" })).toBeTruthy();
    expect(welcome.textContent).toContain("New accounts only read at first.");
    expect(welcome.textContent).toContain("system keychain");
  });

  it("About shows icon and wordmark with the name as text, and copies the project's address", async () => {
    mockCore();
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "About" }));

    const brand = await screen.findByTestId("about-brand");
    expect(brand.querySelectorAll("svg")).toHaveLength(2);
    expect(brand.textContent).toContain(PRODUCT_NAME);
    expect(screen.getByText(PROJECT_URL.replace("https://", ""))).toBeTruthy();
    expect(screen.getByText(PUBLISHER)).toBeTruthy();

    await user.click(screen.getByTestId("about-copy-project"));
    expect(await navigator.clipboard.readText()).toBe(PROJECT_URL);
    expect(await screen.findByText("Address copied.")).toBeTruthy();
  });
});
