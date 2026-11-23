import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import tokensCss from "../src/styles/tokens.css?raw";
import { Avatar } from "../src/features/accounts/Avatar";
import { senderTint, senderTintIndex, TINT_COUNT } from "../src/features/accounts/senderTint";
import { contrast } from "./support/color";
import { makeAccount, makeMessage, makeView, mockCore, standardMailboxes } from "./support/core";
import { renderApp } from "./support/render";

/*
 * Decision 0031, "Avatar-Tönungen": initials on a soft tint chosen per sender by a stable hash of
 * the address, the same in every list, window, theme and platform; the account ring only where it
 * tells accounts apart. The contrast of every tint is also part of tests/contrast.test.ts.
 */

function tintTokens(selector: RegExp): Map<string, string> {
  const block = selector.exec(tokensCss)?.[1] ?? "";
  return new Map([...block.matchAll(/--ui-(avatar-\d-(?:bg|text)):\s*(#[0-9a-f]{6});/g)].map((m) => [m[1] ?? "", m[2] ?? ""]));
}

describe("sender tints", () => {
  it("are stable per address, whatever its case or surrounding space, and ignore the name", () => {
    const index = senderTintIndex("grace@example.org");
    expect(senderTintIndex("Grace@Example.ORG")).toBe(index);
    expect(senderTintIndex("  grace@example.org ")).toBe(index);
    expect(senderTintIndex("grace@example.org", "Grace Hopper")).toBe(index);
    expect(senderTintIndex("grace@example.org", "Someone else")).toBe(index);
    // FNV-1a is fixed: the same sender keeps its colour across releases and both platforms.
    expect(senderTintIndex("grace@example.org")).toBe(senderTintIndex("grace@example.org"));
    expect(senderTint("grace@example.org")).toBe(`bg-avatar-${String(index + 1)} text-avatar-${String(index + 1)}-text`);
  });

  it("fall back to the name without an address and spread senders over all eight tints", () => {
    expect(senderTintIndex("", "Billing")).toBe(senderTintIndex("", "billing"));
    const used = new Set(Array.from({ length: 64 }, (_, i) => senderTintIndex(`person${String(i)}@example.org`)));
    expect(TINT_COUNT).toBe(8);
    expect(used.size).toBe(8);
    for (const index of used) expect(index).toBeGreaterThanOrEqual(0);
  });

  it("reach 4.5:1 for the initials on their fill, in light and dark", () => {
    const light = tintTokens(/:root\s*\{([^}]*)\}/);
    const dark = tintTokens(/:root\[data-theme="dark"\]\s*\{([^}]*)\}/);
    for (const theme of [light, dark]) {
      expect(theme.size).toBe(16);
      for (let n = 1; n <= TINT_COUNT; n += 1) {
        expect(contrast(theme.get(`avatar-${String(n)}-text`) ?? "", theme.get(`avatar-${String(n)}-bg`) ?? "")).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});

describe("avatar", () => {
  it("draws the initials on the sender's tint, the account ring only when asked", () => {
    render(
      <>
        <Avatar name="Grace Hopper" email="grace@example.org" accountColor={2} />
        <Avatar name="Grace Hopper" email="grace@example.org" accountColor={2} ring={false} />
        <Avatar name="Grace Hopper" email="grace@example.org" accountColor={2} size="mini" />
      </>,
    );
    const [ringed, plain, mini] = screen.getAllByTestId("avatar");
    const tint = senderTint("grace@example.org");
    for (const avatar of [ringed, plain, mini]) {
      expect(avatar?.getAttribute("aria-hidden")).toBe("true");
      for (const name of tint.split(" ")) expect(avatar?.className).toContain(name);
    }
    expect(ringed?.textContent).toBe("GH");
    expect(ringed?.className).toContain("ring-account-3");
    expect(plain?.className).not.toContain("ring-");
    // The 16 px chip avatar holds one letter and never a ring.
    expect(mini?.textContent).toBe("G");
    expect(mini?.className).not.toContain("ring-");
  });

  it("leaves the account ring out of a single account's list, where every row would carry it", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    const items = [
      makeMessage({ id: 2, from: { name: "Grace Hopper", email: "grace@example.org" }, subject: "One" }),
      makeMessage({ id: 1, from: { name: "Alan Turing", email: "alan@example.org" }, subject: "Two", date: Date.now() - 60_000 }),
    ];
    mockCore({
      accounts: [makeAccount({ color: 4 })],
      mailboxes: standardMailboxes(),
      messages: { 1: items },
      views: Object.fromEntries(items.map((item) => [item.id, makeView(item)])),
    });
    renderApp();
    const rows = await screen.findAllByRole("option");
    const avatars = rows.map((row) => within(row).getByTestId("avatar"));
    expect(avatars.map((avatar) => avatar.className.includes("ring-account"))).toEqual([false, false]);
    expect(avatars[0]?.className).toContain(senderTint("grace@example.org").split(" ")[0]);
    expect(avatars[1]?.className).toContain(senderTint("alan@example.org").split(" ")[0]);
  });
});
