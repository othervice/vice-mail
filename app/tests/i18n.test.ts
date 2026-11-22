import { describe, expect, it } from "vitest";
import { de } from "../src/i18n/de";
import { en } from "../src/i18n/en";

function keys(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([key, child]) => keys(child, prefix ? `${prefix}.${key}` : key));
}

function leaves(value: unknown): unknown[] {
  if (typeof value !== "object" || value === null) return [value];
  return Object.values(value).flatMap(leaves);
}

describe("translations", () => {
  it("German and English have exactly the same keys", () => {
    expect(keys(de).sort()).toEqual(keys(en).sort());
  });

  it("every text is a non-empty string", () => {
    for (const text of [...leaves(en), ...leaves(de)]) {
      expect(typeof text).toBe("string");
      expect((text as string).trim()).not.toBe("");
    }
  });
});
