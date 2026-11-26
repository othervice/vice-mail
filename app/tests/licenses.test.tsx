import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import * as licenses from "../src/features/settings/licenses";
import { groupLicenses, licenseTextTitle, parseLicenseData, type LicensePackage } from "../src/features/settings/licenses";
import generated from "../src/generated/licenses.json?raw";
import { focusRingReport } from "./support/focusRing";
import { mockCore } from "./support/core";
import { renderApp } from "./support/render";

// Stage 15, "Lizenzen": the list of libraries with their licences in Settings → About.

function pkg(overrides: Partial<LicensePackage> = {}): LicensePackage {
  return {
    name: "example",
    version: "1.0.0",
    ecosystem: "rust",
    license: "MIT",
    declared: "MIT OR Apache-2.0",
    repository: "https://github.com/example/example",
    notices: ["Copyright (c) 2024 Example Authors"],
    texts: ["MIT"],
    ...overrides,
  };
}

async function openLicenses(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(screen.getByRole("tab", { name: "About" }));
  const panel = screen.getByRole("tabpanel", { name: "About" });
  const toggle = within(panel).getByRole("button", { name: "Show libraries and licenses" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(within(panel).queryByTestId("about-licenses")).toBeNull();
  await user.click(toggle);
  const list = await within(panel).findByTestId("about-licenses");
  await within(list).findByRole("searchbox", { name: "Search libraries" });
  return { panel, list, toggle };
}

describe("licence data", () => {
  it("parses the generated file: every package names a licence and has its texts", () => {
    const data = parseLicenseData(generated);
    expect(data.packages.length).toBeGreaterThan(100);
    const names = new Set(data.packages.map((p) => `${p.ecosystem}:${p.name}`));
    // The shell, the TLS stack, the database and the frontend's framework are all listed.
    for (const name of ["rust:tauri", "rust:rustls", "rust:aws-lc-sys", "rust:libsqlite3-sys", "npm:react", "npm:@tiptap/core"]) {
      expect(names.has(name), name).toBe(true);
    }
    // Our own crates and the app itself are not third-party libraries; dev tools are not shipped.
    for (const name of ["rust:mail-app", "rust:mail-engine", "npm:app", "npm:vite", "npm:vitest", "rust:rmcp"]) {
      expect(names.has(name), name).toBe(false);
    }
    for (const p of data.packages) {
      expect(p.texts.length, p.name).toBeGreaterThan(0);
      expect(p.license).not.toMatch(/GPL/);
    }
    // Stage 22 (decision 0080): the font of the unread count on Windows, with the OFL's text.
    const font = data.packages.find((p) => p.ecosystem === "font");
    expect(font?.license).toBe("OFL-1.1");
    expect(font?.platforms).toEqual(["windows"]);
    expect(font?.notices.join(" ")).toMatch(/Microsoft Corporation/);
    expect(data.texts["OFL-1.1"]).toMatch(/SIL OPEN FONT LICENSE Version 1\.1/);
  });

  it("rejects malformed data", () => {
    expect(() => parseLicenseData("[]")).toThrow();
    expect(() => parseLicenseData(JSON.stringify({ packages: [], texts: { MIT: 1 } }))).toThrow();
    expect(() => parseLicenseData(JSON.stringify({ packages: [pkg()], texts: {} }))).toThrow(/entry 0/);
    expect(() => parseLicenseData(JSON.stringify({ packages: [pkg({ platforms: ["linux" as never] })], texts: { MIT: "x" } }))).toThrow();
    expect(parseLicenseData(JSON.stringify({ packages: [pkg()], texts: { MIT: "x" } })).packages).toHaveLength(1);
  });

  it("groups by licence, largest group first, and filters by name and licence", () => {
    const packages = [
      pkg({ name: "alpha" }),
      pkg({ name: "beta", license: "Apache-2.0", declared: "Apache-2.0" }),
      pkg({ name: "gamma" }),
      pkg({ name: "delta-gam", license: "BSD-3-Clause", declared: "BSD-3-Clause" }),
    ];
    expect(groupLicenses(packages, "").map((g) => [g.license, g.packages.map((p) => p.name)])).toEqual([
      ["MIT", ["alpha", "gamma"]],
      ["Apache-2.0", ["beta"]],
      ["BSD-3-Clause", ["delta-gam"]],
    ]);
    // Groups of the same size by licence name.
    expect(groupLicenses(packages, "  GAM ").map((g) => g.packages.map((p) => p.name))).toEqual([["delta-gam"], ["gamma"]]);
    // The licence and the declared licence count too: "apache" finds the MIT OR Apache-2.0 packages.
    expect(groupLicenses(packages, "apache").flatMap((g) => g.packages.map((p) => p.name))).toEqual(["alpha", "gamma", "beta"]);
    expect(groupLicenses(packages, "bsd").flatMap((g) => g.packages.map((p) => p.name))).toEqual(["delta-gam"]);
    expect(groupLicenses(packages, "nothing")).toEqual([]);
  });

  it("names licence texts without their variant number", () => {
    expect(licenseTextTitle("MIT")).toEqual({ kind: "license", name: "MIT" });
    expect(licenseTextTitle("Apache-2.0 #3")).toEqual({ kind: "license", name: "Apache-2.0" });
    expect(licenseTextTitle("Apache-2.0 + ISC")).toEqual({ kind: "license", name: "Apache-2.0, ISC" });
    expect(licenseTextTitle("Notice #12")).toEqual({ kind: "notice" });
  });
});

describe("Settings → About, open-source licenses", () => {
  it("opens the list on request, grouped by licence, and finds a library", async () => {
    mockCore();
    const { user } = renderApp();
    const { list, toggle } = await openLicenses(user);
    const total = parseLicenseData(generated).packages.length;

    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(toggle.getAttribute("aria-controls")).toBe(list.id);
    expect(toggle.textContent).toBe("Hide libraries and licenses");
    expect(within(list).getByRole("status").textContent).toBe(`${total.toLocaleString("en")} libraries`);
    // MIT is the largest group and comes first.
    const groups = within(list).getAllByRole("heading", { level: 3 });
    expect(groups[0]?.textContent).toMatch(/^MIT \(\d+\)$/);

    await user.type(within(list).getByRole("searchbox", { name: "Search libraries" }), "rustls-platform");
    await waitFor(() => {
      expect(within(list).getByRole("status").textContent).toBe(`1 of ${total.toLocaleString("en")} libraries`);
    });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);

    await user.clear(within(list).getByRole("searchbox", { name: "Search libraries" }));
    await user.type(within(list).getByRole("searchbox", { name: "Search libraries" }), "no-such-library");
    expect(await within(list).findByText("No library matches “no-such-library”.")).toBeTruthy();
    expect(within(list).queryAllByRole("listitem")).toHaveLength(0);
  });

  it("opens an entry with the keyboard: licence, declared licence, copyright lines, texts and a copyable address", async () => {
    mockCore();
    const { user } = renderApp();
    const { list } = await openLicenses(user);
    const data = parseLicenseData(generated);
    const reactDom = data.packages.find((p) => p.ecosystem === "npm" && p.name === "react-dom");
    expect(reactDom?.repository).toMatch(/^https:\/\/github\.com\//);
    if (!reactDom?.repository) return;

    await user.type(within(list).getByRole("searchbox", { name: "Search libraries" }), "react-dom");
    const entry = await within(list).findByTestId(`about-license-npm-react-dom-${reactDom.version}`);
    expect(entry.getAttribute("aria-expanded")).toBe("false");
    entry.focus();
    await user.keyboard("{Enter}");
    expect(entry.getAttribute("aria-expanded")).toBe("true");
    const details = within(list).getByTestId("about-license-details");
    expect(entry.getAttribute("aria-controls")).toBe(details.id);
    expect(details.textContent).toContain("MIT");
    expect(details.textContent).toContain("Copyright (c) Meta Platforms, Inc. and affiliates.");
    expect(within(details).getByRole("heading", { name: "License text: MIT" })).toBeTruthy();
    expect(details.querySelector("pre")?.textContent).toContain("Permission is hereby granted, free of charge");
    // The address is text with a copy button, never a link the web view could open.
    expect(details.querySelector("a")).toBeNull();
    await user.click(within(details).getByRole("button", { name: "Copy the address of react-dom" }));
    expect(await navigator.clipboard.readText()).toBe(reactDom.repository);

    await user.keyboard(" ");
    entry.focus();
    await user.keyboard(" ");
    expect(entry.getAttribute("aria-expanded")).toBe("false");
    expect(within(list).queryByTestId("about-license-details")).toBeNull();
  });

  it("shows which build a platform-specific crate is in and the licence it is chosen under", async () => {
    mockCore();
    const { user } = renderApp();
    const { list } = await openLicenses(user);
    const data = parseLicenseData(generated);
    const windowsOnly = data.packages.find((p) => p.name === "windows-sys");
    expect(windowsOnly?.platforms).toEqual(["windows"]);
    if (!windowsOnly) return;

    await user.type(within(list).getByRole("searchbox", { name: "Search libraries" }), "windows-sys");
    await user.click(await within(list).findByTestId(`about-license-rust-windows-sys-${windowsOnly.version}`));
    const details = within(list).getByTestId("about-license-details");
    expect(details.textContent).toContain("Only in the build for");
    expect(details.textContent).toContain("Windows");
    expect(details.textContent).toContain(`Declared by the library${windowsOnly.declared}`);
  });

  it("speaks German", async () => {
    mockCore({ preferences: { theme: "light", language: "de" } });
    const { user } = renderApp("de");
    await user.click(await screen.findByRole("button", { name: "Einstellungen" }));
    await user.click(screen.getByRole("tab", { name: "Über" }));
    await user.click(screen.getByRole("button", { name: "Bibliotheken und Lizenzen zeigen" }));
    const list = await screen.findByTestId("about-licenses");
    expect(await within(list).findByRole("searchbox", { name: "Bibliotheken durchsuchen" })).toBeTruthy();
    expect(within(list).getByRole("status").textContent).toMatch(/^[\d.]+ Bibliotheken$/);
  });

  it("shows a failed load with a way to try again", async () => {
    const load = vi.spyOn(licenses, "loadLicenseData").mockRejectedValueOnce(new Error("chunk missing"));
    mockCore();
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("tab", { name: "About" }));
    await user.click(screen.getByRole("button", { name: "Show libraries and licenses" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The list of libraries and licenses could not be loaded.");
    expect(alert.textContent).toContain("chunk missing");
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("searchbox", { name: "Search libraries" })).toBeTruthy();
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("shows the focus ring on everything in the open list", async () => {
    mockCore();
    const { user } = renderApp();
    const { list } = await openLicenses(user);
    await user.type(within(list).getByRole("searchbox", { name: "Search libraries" }), "react-dom");
    await user.click(await within(list).findByRole("button", { name: /^react-dom/ }));
    const report = await focusRingReport(list);
    expect(report.findings).toEqual([]);
    // Search field, the entry and its copy button.
    expect(report.checked).toBeGreaterThanOrEqual(3);
  });
});
