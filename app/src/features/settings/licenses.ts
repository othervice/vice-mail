/*
 * The third-party licences of the app (stage 15): `src/generated/licenses.json`, written by
 * `scripts/licenses.mjs` from the Rust crates and npm packages the app is built from. Loaded only
 * when the list is opened (a chunk of its own, about 370 kB of text), as raw text through Vite, so
 * the type checker never reads the whole file. Plain data: no IPC, no network.
 */

export type LicenseEcosystem = "rust" | "npm";
export type LicensePlatform = "macos" | "windows";

export interface LicensePackage {
  name: string;
  version: string;
  ecosystem: LicenseEcosystem;
  /** The licence the package is used under here ("MIT", "MIT AND BSD-3-Clause"). */
  license: string;
  /** What the package declares ("MIT OR Apache-2.0"). */
  declared: string;
  repository: string | null;
  /** Copyright lines from the package's licence files. */
  notices: string[];
  /** Keys into `LicenseData.texts`. */
  texts: string[];
  /** Only in the build for these platforms; absent when in every build. */
  platforms?: LicensePlatform[];
}

export interface LicenseData {
  packages: LicensePackage[];
  texts: Record<string, string>;
}

export interface LicenseGroup {
  license: string;
  packages: LicensePackage[];
}

const isString = (value: unknown): value is string => typeof value === "string";
const isStringArray = (value: unknown): value is string[] => Array.isArray(value) && value.every(isString);

function isPackage(value: unknown, texts: Record<string, string>): value is LicensePackage {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    isString(v["name"]) &&
    isString(v["version"]) &&
    (v["ecosystem"] === "rust" || v["ecosystem"] === "npm") &&
    isString(v["license"]) &&
    isString(v["declared"]) &&
    (v["repository"] === null || isString(v["repository"])) &&
    isStringArray(v["notices"]) &&
    isStringArray(v["texts"]) &&
    v["texts"].every((id) => Object.hasOwn(texts, id)) &&
    (v["platforms"] === undefined ||
      (Array.isArray(v["platforms"]) && v["platforms"].every((p) => p === "macos" || p === "windows")))
  );
}

/** Parses and checks the generated file; throws on anything unexpected. */
export function parseLicenseData(raw: string): LicenseData {
  const data: unknown = JSON.parse(raw);
  if (typeof data !== "object" || data === null) throw new Error("licenses.json: not an object");
  const record = data as Record<string, unknown>;
  const texts = record["texts"];
  if (typeof texts !== "object" || texts === null || !Object.values(texts).every(isString)) {
    throw new Error("licenses.json: `texts` is not a map of strings");
  }
  const textMap = texts as Record<string, string>;
  const packages = record["packages"];
  if (!Array.isArray(packages)) throw new Error("licenses.json: `packages` is not a list");
  const bad = packages.findIndex((entry) => !isPackage(entry, textMap));
  if (bad >= 0) throw new Error(`licenses.json: entry ${bad} is malformed`);
  return { packages: packages as LicensePackage[], texts: textMap };
}

/** Loads the list (a separate chunk). */
export async function loadLicenseData(): Promise<LicenseData> {
  // Static data, not the IPC bindings that only services/ipc.ts may import (eslint.config.js).
  const module = await import("../../generated/licenses.json?raw");
  return parseLicenseData(module.default);
}

/**
 * The packages matching `query` (name, licence or declared licence, case-insensitive), grouped by
 * the licence they are used under: the largest group first, then by name.
 */
export function groupLicenses(packages: readonly LicensePackage[], query: string): LicenseGroup[] {
  const needle = query.trim().toLocaleLowerCase();
  const groups = new Map<string, LicensePackage[]>();
  for (const pkg of packages) {
    if (
      needle &&
      !pkg.name.toLocaleLowerCase().includes(needle) &&
      !pkg.license.toLocaleLowerCase().includes(needle) &&
      !pkg.declared.toLocaleLowerCase().includes(needle)
    ) {
      continue;
    }
    const group = groups.get(pkg.license);
    if (group) group.push(pkg);
    else groups.set(pkg.license, [pkg]);
  }
  return [...groups]
    .map(([license, list]) => ({ license, packages: list }))
    .sort((a, b) => b.packages.length - a.packages.length || a.license.localeCompare(b.license, "en"));
}

/** The heading of a licence text: its key without the variant number ("MIT #3" → "MIT"). */
export function licenseTextTitle(id: string): { kind: "license"; name: string } | { kind: "notice" } {
  const name = id.replace(/ #\d+$/, "");
  return name === "Notice" ? { kind: "notice" } : { kind: "license", name: name.replaceAll(" + ", ", ") };
}
