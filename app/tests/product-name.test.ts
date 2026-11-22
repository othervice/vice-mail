import { expect, it } from "vitest";
import indexHtml from "../index.html?raw";
import { PRODUCT_NAME } from "../src/config/product";

// The product name is a working title: it may only be written in src/config/product.ts (and in
// src-tauri/tauri.conf.json), so renaming the app is a two-line change.
const sources = import.meta.glob<string>(["../src/**/*.{ts,tsx,css}", "!../src/config/product.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

it("is written nowhere in the frontend except config/product.ts", () => {
  expect(Object.keys(sources).length).toBeGreaterThan(10);
  const offenders = Object.entries(sources)
    .filter(([, text]) => text.includes(PRODUCT_NAME))
    .map(([path]) => path);
  expect(offenders).toEqual([]);
  expect(indexHtml).not.toContain(PRODUCT_NAME);
});
