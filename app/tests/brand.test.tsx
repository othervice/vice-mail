import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppIcon, Wordmark } from "../src/components/Brand";
import { PRODUCT_NAME, PROJECT_URL } from "../src/config/product";
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

    await user.click(screen.getByTestId("about-copy-project"));
    expect(await navigator.clipboard.readText()).toBe(PROJECT_URL);
    expect(await screen.findByText("Address copied.")).toBeTruthy();
  });
});
