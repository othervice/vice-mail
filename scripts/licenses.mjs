#!/usr/bin/env node
// Third-party licences of everything the app ships, for the About view and the installers.
//
//   node scripts/licenses.mjs           write app/src/generated/licenses.json and
//                                       app/src/generated/THIRD-PARTY-NOTICES.txt
//   node scripts/licenses.mjs --check   fail if either file is out of date or a licence is not
//                                       allowed (part of scripts/check.sh)
//
// No network: Rust crates come from `cargo tree`/`cargo metadata --offline --locked` and their
// sources in the Cargo registry cache (`cargo fetch --locked` once); npm packages from
// app/node_modules (`pnpm --dir app install --frozen-lockfile`).
//
// What counts as shipped:
// - Files that are no package (ASSETS below): the UI font, the font of the unread count on Windows.
// - Rust: the crates linked into the `mail-app` and `mail-mcp` binaries for the targets of
//   deny.toml ([graph] targets): normal dependencies with default features, without build
//   scripts' dependencies, dev-dependencies and proc-macros (they run in the compiler and are not
//   part of the binary). Our own workspace crates are the app itself and are left out.
// - npm: the production dependencies of app/package.json and their dependencies (and required
//   peer dependencies), i.e. what Vite can bundle; never devDependencies or type packages.
//
// Licences: each package's SPDX expression must be satisfiable with the allow-list of deny.toml
// ([licenses] allow, same logic as cargo-deny), plus NPM_EXTRA_ALLOW for the npm side. For a
// choice ("MIT OR Apache-2.0") the list names the licence used (fewest licences, then the order of
// the allow-list). The texts come from the packages' own LICENSE/COPYING/NOTICE files: copyright
// lines stay with the package, the remaining text is stored once and shared.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const APP = join(ROOT, "app");
const OUT_JSON = join(APP, "src", "generated", "licenses.json");
const OUT_TEXT = join(APP, "src", "generated", "THIRD-PARTY-NOTICES.txt");
const RUST_BINARIES = ["mail-app", "mail-mcp"];

// npm packages may need licences no Rust crate uses (cargo-deny warns about unused entries, so
// they are not in deny.toml). Each one is GPL-3.0 compatible; keep the reason next to it.
const NPM_EXTRA_ALLOW = [
  "0BSD", // tslib (via radix-ui's aria-hidden, react-remove-scroll): public-domain-like, no conditions
];

// Files the app embeds that come from no package manager, each with its licence file in `dir`.
const ASSETS = [
  {
    // Stage 23 (decision 0083): the UI font, served by the app from app/src/assets/fonts. The
    // unmodified InterVariable.woff2 of the release (Inter-4.1.zip, web/; SHA-256 checked by
    // app/tests/font.test.ts), in every build.
    ecosystem: "font",
    name: "Inter",
    version: "4.1",
    declared: "OFL-1.1",
    repository: "https://github.com/rsms/inter",
    dir: join(APP, "src", "assets", "fonts"),
    platforms: null,
  },
  {
    // Decision 0080: the digits of the unread count on Windows (taskbar overlay, notification
    // area), a subset of Selawik Semibold written by scripts/badge-font.py, embedded in mail-app.
    ecosystem: "font",
    name: "Badge Digits (from Selawik Semibold)",
    version: "1.01",
    declared: "OFL-1.1",
    repository: "https://github.com/microsoft/Selawik",
    dir: join(APP, "src-tauri", "fonts"),
    platforms: ["windows"],
  },
];
// Licences only fonts use. The SIL Open Font License lets the font be bundled and embedded in any
// software; the font itself stays under the OFL (an aggregate, not a combined work with the GPL).
const FONT_EXTRA_ALLOW = ["OFL-1.1"];

// --- small helpers -----------------------------------------------------------------------------

function fail(message) {
  console.error(`licenses: ${message}`);
  process.exit(1);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function run(command, args, options = {}) {
  try {
    return execFileSync(command, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...options });
  } catch (error) {
    const stderr = error.stderr ? String(error.stderr).trim() : String(error);
    fail(`\`${command} ${args.join(" ")}\` failed:\n${stderr}\n(missing crate sources? run \`cargo fetch --locked\` once)`);
  }
}

const compareText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

// --- allow-list (deny.toml) --------------------------------------------------------------------

function denyToml() {
  const text = readFileSync(join(ROOT, "deny.toml"), "utf8");
  const section = (name) => {
    const start = text.indexOf(`\n[${name}]`);
    if (start < 0) fail(`deny.toml has no [${name}] section`);
    const rest = text.slice(start + name.length + 3);
    const end = rest.search(/\n\[/);
    return end < 0 ? rest : rest.slice(0, end);
  };
  const stringArray = (block, key) => {
    const match = block.match(new RegExp(`^${key}\\s*=\\s*\\[([\\s\\S]*?)\\]`, "m"));
    if (!match) fail(`deny.toml: no \`${key}\` list`);
    // Strip comments, then take the quoted strings.
    const body = match[1].replace(/#.*$/gm, "");
    return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  };
  return { allow: stringArray(section("licenses"), "allow"), targets: stringArray(section("graph"), "targets") };
}

// --- SPDX expressions --------------------------------------------------------------------------

/** Parses an SPDX expression into its alternatives: a list of licence sets (OR of ANDs). */
function spdxAlternatives(expression) {
  // Old Cargo style "MIT/Apache-2.0" means OR.
  const tokens = expression
    .replace(/\s*\/\s*/g, " OR ")
    .replace(/[()]/g, " $& ")
    .split(/\s+/)
    .filter(Boolean);
  let at = 0;
  const peek = () => tokens[at];
  const next = () => tokens[at++];
  const parseOr = () => {
    let left = parseAnd();
    while (peek() === "OR") {
      next();
      left = [...left, ...parseAnd()];
    }
    return left;
  };
  const parseAnd = () => {
    let left = parseAtom();
    while (peek() === "AND") {
      next();
      const right = parseAtom();
      left = left.flatMap((a) => right.map((b) => [...new Set([...a, ...b])].sort(compareText)));
    }
    return left;
  };
  const parseAtom = () => {
    const token = next();
    if (token === undefined) throw new Error("unexpected end");
    if (token === "(") {
      const inner = parseOr();
      if (next() !== ")") throw new Error("missing )");
      return inner;
    }
    if (["AND", "OR", "WITH", ")"].includes(token)) throw new Error(`unexpected ${token}`);
    let id = token;
    if (peek() === "WITH") {
      next();
      const exception = next();
      if (!exception) throw new Error("missing exception after WITH");
      id = `${token} WITH ${exception}`;
    }
    return [[id]];
  };
  const result = parseOr();
  if (at !== tokens.length) throw new Error(`unexpected ${tokens[at]}`);
  return result;
}

/** The allowed alternative with the fewest licences (then the allow-list's order), or null. */
function chooseLicences(expression, allow) {
  const rank = (id) => allow.indexOf(id);
  const candidates = spdxAlternatives(expression).filter((set) => set.every((id) => rank(id) >= 0));
  if (candidates.length === 0) return null;
  const score = (set) => set.reduce((sum, id) => sum + rank(id), 0);
  candidates.sort((a, b) => a.length - b.length || score(a) - score(b));
  return [...candidates[0]].sort((a, b) => rank(a) - rank(b));
}

// --- licence files -----------------------------------------------------------------------------

const LICENSE_FILE = /^(un)?licen[cs]e|^copying|^copyright|^notice/i;

/** A line without a leading comment marker ("// Copyright …" in a LICENSE file written as code). */
const uncomment = (line) => line.replace(/^\s*(\/\/+|#+|\/?\*+|;+)\s?/, "");

/** Which licences a file's text is (by their characteristic sentences). */
function classify(text) {
  const t = text.split(/\r?\n/).map(uncomment).join(" ").replace(/\s+/g, " ");
  const ids = [];
  if (/Apache License,? Version 2\.0/i.test(t) && /TERMS AND CONDITIONS FOR USE, REPRODUCTION/i.test(t)) {
    ids.push(/LLVM Exceptions? to the Apache 2\.0 License/i.test(t) ? "Apache-2.0 WITH LLVM-exception" : "Apache-2.0");
  }
  if (/Permission is hereby granted, free of charge, to any person obtaining a copy of this software/i.test(t)) {
    ids.push(/above copyright notice and this permission notice shall be included/i.test(t) ? "MIT" : "MIT-0");
  }
  if (/Redistribution and use in source and binary forms/i.test(t)) {
    ids.push(/Neither the name|names of (its|the) contributors/i.test(t) ? "BSD-3-Clause" : "BSD-2-Clause");
  }
  if (/Permission to use, copy, modify, and\/?(or)? distribute this software for any purpose/i.test(t)) {
    ids.push(/provided that the above copyright notice and this permission notice appear/i.test(t) ? "ISC" : "0BSD");
  }
  if (/provided ['‘]as-is['’], without any express or implied warranty/i.test(t) && /Altered source versions must be plainly marked/i.test(t)) ids.push("Zlib");
  if (/Mozilla Public License,? (Version|v\.?) ?2\.0/i.test(t) && /Definitions/.test(t)) ids.push("MPL-2.0");
  if (/UNICODE LICENSE V3|Unicode Data Files and Software/i.test(t)) ids.push("Unicode-3.0");
  if (/This is free and unencumbered software released into the public domain/i.test(t)) ids.push("Unlicense");
  if (/Boost Software License - Version 1\.0/i.test(t)) ids.push("BSL-1.0");
  if (/CC0 1\.0 Universal/i.test(t)) ids.push("CC0-1.0");
  if (/SIL OPEN FONT LICENSE,? Version 1\.1/i.test(t)) ids.push("OFL-1.1");
  // Only a file that is a GNU licence, not one that mentions it (MPL-2.0 names the GPL).
  const gnu = t.slice(0, 300).match(/^\s*(?:\S+\s+){0,3}GNU (LESSER |LIBRARY |AFFERO )?GENERAL PUBLIC LICENSE,? Version (\d)/i);
  if (gnu) {
    const kind = { "LESSER ": "LGPL", "LIBRARY ": "LGPL", "AFFERO ": "AGPL" }[gnu[1]?.toUpperCase() ?? ""] ?? "GPL";
    ids.push(`${kind}-${gnu[2]}.${kind === "LGPL" && gnu[2] === "2" ? "1" : "0"}`);
  }
  return ids;
}

const TEMPLATE = /\[yyyy\]|\[year\]|<year>|\{yyyy\}|\[name of copyright owner\]|<copyright holders?>|<OWNER>|\{name of copyright owner\}/i;

/** A copyright line of a package ("Copyright (c) 2014 Jane Doe"), not a sentence of the licence. */
function isCopyrightLine(line) {
  const l = uncomment(line).trim();
  if (!/^(Copyright\b|COPYRIGHT\s*(\(C\)|©|\d)|\([cC]\)\s*\d|©)/.test(l)) return false;
  if (TEMPLATE.test(l)) return false;
  return !/^Copyright\s+(notice|holders?|owners?|license|licen[cs]ing|and|law|statement|protection|is|of the|to|in|by)\b/i.test(l);
}

function splitCopyright(text) {
  const notices = [];
  const body = [];
  for (const line of text.replace(/\r\n?/g, "\n").replace(/\t/g, "    ").split("\n")) {
    if (isCopyrightLine(line)) notices.push(uncomment(line).trim().replace(/\s+/g, " "));
    else body.push(line.replace(/\s+$/, ""));
  }
  const cleaned = body
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/^\n+|\n+$/g, "");
  return { notices, body: cleaned };
}

function licenseFiles(dir) {
  let names;
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  return names
    .filter((name) => LICENSE_FILE.test(name) && !/\.(rs|js|cjs|mjs|ts|json|html?)$/i.test(name))
    .filter((name) => {
      try {
        return statSync(join(dir, name)).isFile();
      } catch {
        return false;
      }
    })
    .sort(compareText)
    .map((name) => ({ name, text: readFileSync(join(dir, name), "utf8") }));
}

// --- Rust --------------------------------------------------------------------------------------

function platformOf(target) {
  if (target.includes("apple")) return "macos";
  if (target.includes("windows")) return "windows";
  return target;
}

function rustPackages(targets) {
  const metadata = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--locked", "--offline"]));
  const byNameVersion = new Map(metadata.packages.map((p) => [`${p.name} ${p.version}`, p]));
  const workspace = new Set(metadata.workspace_members);
  const platforms = new Map(); // "name version" -> Set of platforms
  for (const target of targets) {
    for (const binary of RUST_BINARIES) {
      const out = run("cargo", [
        "tree", "--locked", "--offline", "-p", binary, "--target", target,
        "-e", "normal,no-proc-macro", "--prefix", "none", "--format", "{p}",
      ]);
      for (const raw of out.split("\n")) {
        const line = raw.replace(/ \(\*\)$/, "").trim();
        if (!line) continue;
        const match = line.match(/^(\S+) v(\S+)/);
        if (!match) fail(`cannot read cargo tree line: ${line}`);
        const key = `${match[1]} ${match[2]}`;
        const pkg = byNameVersion.get(key);
        if (!pkg) fail(`cargo tree names ${key}, cargo metadata does not`);
        if (workspace.has(pkg.id)) continue;
        if (!platforms.has(key)) platforms.set(key, new Set());
        platforms.get(key).add(platformOf(target));
      }
    }
  }
  const allPlatforms = new Set(targets.map(platformOf));
  return [...platforms].map(([key, set]) => {
    const pkg = byNameVersion.get(key);
    if (!pkg.source?.startsWith("registry+")) fail(`${key} does not come from crates.io (${pkg.source})`);
    const dir = dirname(pkg.manifest_path);
    const files = licenseFiles(dir);
    if (pkg.license_file && !files.some((f) => join(dir, f.name) === join(dir, pkg.license_file))) {
      const path = join(dir, pkg.license_file);
      if (existsSync(path)) files.push({ name: relative(dir, path), text: readFileSync(path, "utf8") });
    }
    return {
      ecosystem: "rust",
      name: pkg.name,
      version: pkg.version,
      declared: pkg.license ?? null,
      repository: cleanUrl(pkg.repository ?? pkg.homepage ?? ""),
      platforms: set.size === allPlatforms.size ? null : [...set].sort(compareText),
      files,
    };
  });
}

// --- npm ---------------------------------------------------------------------------------------

function resolveNpm(name, fromDir) {
  for (let dir = fromDir; ; dir = dirname(dir)) {
    const candidate = join(dir, "node_modules", name);
    if (existsSync(join(candidate, "package.json"))) return realpathSync(candidate);
    if (dir === APP || dirname(dir) === dir) return null;
  }
}

function npmPackages() {
  if (!existsSync(join(APP, "node_modules"))) fail("app/node_modules is missing; run: pnpm --dir app install --frozen-lockfile");
  const root = readJson(join(APP, "package.json"));
  const seen = new Map(); // realpath -> package
  const queue = Object.keys(root.dependencies ?? {}).map((name) => ({ name, from: APP, optional: false }));
  while (queue.length > 0) {
    const { name, from, optional } = queue.shift();
    if (name.startsWith("@types/")) continue;
    const dir = resolveNpm(name, from);
    if (!dir) {
      if (optional) continue;
      fail(`npm package ${name} (needed from ${relative(ROOT, from)}) is not installed; run: pnpm --dir app install --frozen-lockfile`);
    }
    if (seen.has(dir)) continue;
    const pkg = readJson(join(dir, "package.json"));
    // pnpm installs such packages only on matching hosts: the list (and --check) would then
    // depend on the machine it runs on.
    if (pkg.os || pkg.cpu) fail(`npm package ${pkg.name} is platform-specific (os/cpu); decide here how to list it`);
    seen.set(dir, pkg);
    for (const dep of Object.keys(pkg.dependencies ?? {})) queue.push({ name: dep, from: dir, optional: false });
    for (const dep of Object.keys(pkg.optionalDependencies ?? {})) queue.push({ name: dep, from: dir, optional: true });
    const peerMeta = pkg.peerDependenciesMeta ?? {};
    for (const dep of Object.keys(pkg.peerDependencies ?? {})) {
      if (!peerMeta[dep]?.optional) queue.push({ name: dep, from: dir, optional: false });
    }
  }
  return [...seen].map(([dir, pkg]) => {
    let declared = typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? null);
    if (!declared && Array.isArray(pkg.licenses)) declared = pkg.licenses.map((l) => l.type ?? l).join(" OR ");
    const repo = typeof pkg.repository === "string" ? pkg.repository : (pkg.repository?.url ?? pkg.homepage ?? "");
    return {
      ecosystem: "npm",
      name: pkg.name,
      version: pkg.version,
      declared: declared || null,
      repository: cleanUrl(repo),
      platforms: null,
      files: licenseFiles(dir),
    };
  });
}

function cleanUrl(url) {
  let u = String(url).trim();
  if (!u) return null;
  u = u.replace(/^git\+/, "").replace(/^git:\/\//, "https://").replace(/^ssh:\/\/git@/, "https://");
  u = u.replace(/^git@([^:]+):/, "https://$1/");
  if (/^github:/.test(u)) u = `https://github.com/${u.slice(7)}`;
  if (/^[\w.-]+\/[\w.-]+$/.test(u)) u = `https://github.com/${u}`;
  return u.replace(/\.git$/, "").replace(/\/+$/, "");
}

// --- assets ------------------------------------------------------------------------------------

function assetPackages() {
  return ASSETS.map(({ dir, ...asset }) => {
    const files = licenseFiles(dir);
    if (files.length === 0) fail(`${asset.name}: no licence file in ${relative(ROOT, dir)}`);
    return { ...asset, files };
  });
}

// --- assembling --------------------------------------------------------------------------------

function build() {
  const { allow, targets } = denyToml();
  const packages = [...rustPackages(targets), ...npmPackages(), ...assetPackages()].sort(
    (a, b) =>
      compareText(a.name.toLowerCase(), b.name.toLowerCase()) ||
      compareText(a.name, b.name) ||
      compareText(a.version, b.version) ||
      compareText(a.ecosystem, b.ecosystem),
  );

  const problems = [];
  // Licence texts, shared: normalised body -> { ids, body, count, order }.
  const variants = new Map();
  const variantOf = (ids, body) => {
    const key = createHash("sha256").update(body.replace(/\s+/g, " ").trim()).digest("hex");
    let variant = variants.get(key);
    if (!variant) {
      variant = { ids, body, count: 0, order: variants.size };
      variants.set(key, variant);
    }
    variant.count += 1;
    return variant;
  };

  const entries = packages.map((pkg) => {
    const label = `${pkg.name} ${pkg.version} (${pkg.ecosystem})`;
    if (!pkg.declared) {
      problems.push(`${label}: no licence declared`);
      return null;
    }
    const extra = { npm: NPM_EXTRA_ALLOW, font: FONT_EXTRA_ALLOW }[pkg.ecosystem] ?? [];
    const allowed = [...allow, ...extra];
    let chosen;
    try {
      chosen = chooseLicences(pkg.declared, allowed);
    } catch (error) {
      problems.push(`${label}: cannot read licence expression "${pkg.declared}" (${error.message})`);
      return null;
    }
    if (!chosen) {
      problems.push(`${label}: "${pkg.declared}" is not covered by the allow-list (deny.toml [licenses] allow)`);
      return null;
    }
    const notices = [];
    const texts = [];
    const covered = new Set();
    for (const file of pkg.files) {
      const ids = classify(file.text);
      const isNotice = /^notice/i.test(file.name);
      // A file that is only a licence we did not choose (LICENSE-APACHE next to LICENSE-MIT).
      if (!isNotice && ids.length > 0 && !ids.some((id) => chosen.includes(id))) continue;
      const split = splitCopyright(file.text);
      for (const notice of split.notices) if (!notices.includes(notice)) notices.push(notice);
      if (!split.body) continue;
      const variant = variantOf(isNotice ? [] : ids, split.body);
      if (!texts.includes(variant)) texts.push(variant);
      for (const id of ids) covered.add(id);
    }
    return { pkg, chosen, notices, texts, missing: chosen.filter((id) => !covered.has(id)) };
  });

  if (problems.length > 0) {
    fail(`licences not allowed or unreadable (GPL-3.0-or-later needs compatible licences):\n  ${problems.join("\n  ")}`);
  }

  // Name the shared texts: the most common text of a licence is its SPDX id, others get a number.
  const ordered = [...variants.values()].sort((a, b) => b.count - a.count || a.order - b.order);
  const used = new Map();
  for (const variant of ordered) {
    const base = variant.ids.length > 0 ? variant.ids.join(" + ") : "Notice";
    const n = (used.get(base) ?? 0) + 1;
    used.set(base, n);
    variant.id = n === 1 && variant.ids.length > 0 ? base : `${base} #${n}`;
  }
  // The reference text of each licence, for packages that ship no licence file of their own.
  const reference = new Map();
  for (const variant of ordered) {
    if (variant.ids.length === 1 && !reference.has(variant.ids[0])) reference.set(variant.ids[0], variant);
  }

  const missingText = [];
  const out = entries.map(({ pkg, chosen, notices, texts, missing }) => {
    for (const id of missing) {
      const fallback = reference.get(id);
      if (fallback) texts.push(fallback);
      else missingText.push(`${pkg.name} ${pkg.version}: no text for ${id}`);
    }
    const entry = {
      name: pkg.name,
      version: pkg.version,
      ecosystem: pkg.ecosystem,
      license: chosen.join(" AND "),
      declared: pkg.declared,
      repository: pkg.repository,
      notices,
      texts: texts.map((v) => v.id),
    };
    if (pkg.platforms) entry.platforms = pkg.platforms;
    return entry;
  });
  if (missingText.length > 0) fail(`no licence text found:\n  ${missingText.join("\n  ")}`);

  const usedTexts = new Set(out.flatMap((e) => e.texts));
  const texts = Object.fromEntries(
    ordered.filter((v) => usedTexts.has(v.id)).sort((a, b) => compareText(a.id, b.id)).map((v) => [v.id, v.body]),
  );
  return { packages: out, texts };
}

function renderJson(data) {
  return `${JSON.stringify({ generatedBy: "scripts/licenses.mjs", ...data }, null, 1)}\n`;
}

function renderText({ packages, texts }) {
  const lines = [
    "Third-party software in this application",
    "",
    "Generated by scripts/licenses.mjs from the packages the application is built from: Rust",
    "crates (Cargo), JavaScript packages (npm) and embedded fonts. Each entry names the licence under",
    "which the package is used here, the licence the package declares, its copyright notices and the",
    "licence texts that apply (the texts follow the list).",
    "",
  ];
  for (const p of packages) {
    lines.push("=".repeat(78));
    const kind = { rust: "Rust crate", npm: "npm package", font: "font" }[p.ecosystem];
    lines.push(`${p.name} ${p.version} (${kind})`);
    lines.push(`Licence: ${p.license}${p.declared !== p.license ? ` (declared: ${p.declared})` : ""}`);
    if (p.repository) lines.push(`Source: ${p.repository}`);
    if (p.platforms) lines.push(`Only on: ${p.platforms.join(", ")}`);
    for (const notice of p.notices) lines.push(notice);
    lines.push(`Licence text: ${p.texts.join("; ")}`);
  }
  lines.push("=".repeat(78), "", "LICENCE TEXTS", "");
  for (const [id, body] of Object.entries(texts)) {
    lines.push("-".repeat(78), id, "-".repeat(78), "", body, "");
  }
  return `${lines.join("\n").replace(/\n+$/, "")}\n`;
}

function summary({ packages }) {
  const counts = new Map();
  for (const p of packages) counts.set(p.license, (counts.get(p.license) ?? 0) + 1);
  const rust = packages.filter((p) => p.ecosystem === "rust").length;
  const npm = packages.filter((p) => p.ecosystem === "npm").length;
  const rows = [...counts].sort((a, b) => b[1] - a[1] || compareText(a[0], b[0]));
  return `${packages.length} packages (${rust} Rust crates, ${npm} npm packages, ${packages.length - rust - npm} fonts)\n${rows
    .map(([license, n]) => `  ${String(n).padStart(4)}  ${license}`)
    .join("\n")}`;
}

const data = build();
const json = renderJson(data);
const text = renderText(data);
if (process.argv.includes("--check")) {
  // A checkout with CRLF line endings (git on Windows) is still up to date.
  const stale = [
    [OUT_JSON, json],
    [OUT_TEXT, text],
  ].filter(([path, content]) => !existsSync(path) || readFileSync(path, "utf8").replace(/\r\n/g, "\n") !== content);
  if (stale.length > 0) {
    fail(`out of date: ${stale.map(([p]) => relative(ROOT, p)).join(", ")}\nrun: node scripts/licenses.mjs`);
  }
  console.log(`licenses: up to date, every licence allowed; ${summary(data).split("\n")[0]}`);
} else {
  writeFileSync(OUT_JSON, json);
  writeFileSync(OUT_TEXT, text);
  console.log(`licenses: wrote ${relative(ROOT, OUT_JSON)} and ${relative(ROOT, OUT_TEXT)}\n${summary(data)}`);
}
