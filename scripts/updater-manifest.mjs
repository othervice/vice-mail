#!/usr/bin/env node
// The update manifest `latest.json` (decision 0037) and the check of update signatures.
//
//   node scripts/updater-manifest.mjs verify <artifact> <version>
//       checks <artifact>.sig against plugins.updater.pubkey in app/src-tauri/tauri.conf.json
//       (or VICEMAIL_UPDATER_PUBKEY: the old key while a bridge release is signed with it):
//       the minisign signature (Ed25519 over BLAKE2b-512), its trusted comment, and that the
//       comment binds the file to <version> (what `requireSignedVersion` demands in the app)
//
//   node scripts/updater-manifest.mjs manifest --version <v> --base-url <url> --out <file>
//       [--notes <text> | --notes-file <file>] [--pub-date <RFC 3339>]
//       [--darwin-aarch64 <artifact>] [--windows-x86_64 <artifact>]
//       writes the static manifest the app reads: version, notes, pub_date, and per platform
//       the download URL (<base-url>/<file name of the artifact>) and the signature (the
//       contents of <artifact>.sig). Every artifact is verified first; nothing is written if
//       one fails.
//
// Platform keys: `darwin-aarch64` is the `.app.tar.gz` of the macOS build, `windows-x86_64` the
// NSIS `-setup.exe` itself (Tauri's v2 updater artifacts). No secret is read or written here.
import { createHash, createPublicKey, verify } from "node:crypto";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const confPath = join(repo, "app", "src-tauri", "tauri.conf.json");
const SEMVER = /^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;
// DER prefix of an Ed25519 SubjectPublicKeyInfo; the raw 32-byte key follows.
const ED25519_SPKI = Buffer.from("302a300506032b6570032100", "hex");

function fail(message) {
  process.stderr.write(`updater-manifest: ${message}\n`);
  process.exit(1);
}

/** The two-line minisign text inside Tauri's base64 wrapping (`.sig` files and `pubkey`). */
function unwrap(base64, what) {
  const text = Buffer.from(base64.trim(), "base64").toString("utf8");
  const lines = text.split("\n").filter((line) => line.length > 0);
  if (lines.length < 2 || !lines[0].startsWith("untrusted comment:")) fail(`${what} is not a minisign text`);
  return lines;
}

function publicKey(
  pubkeyBase64 = process.env.VICEMAIL_UPDATER_PUBKEY || JSON.parse(readFileSync(confPath, "utf8")).plugins?.updater?.pubkey,
) {
  if (typeof pubkeyBase64 !== "string" || pubkeyBase64 === "") fail(`no plugins.updater.pubkey in ${confPath}`);
  const raw = Buffer.from(unwrap(pubkeyBase64, "the public key")[1], "base64");
  if (raw.length !== 42 || raw.subarray(0, 2).toString() !== "Ed") fail("the public key is not a minisign Ed25519 key");
  return {
    keyId: raw.subarray(2, 10),
    key: createPublicKey({ key: Buffer.concat([ED25519_SPKI, raw.subarray(10)]), format: "der", type: "spki" }),
  };
}

/** Verifies `<artifact>.sig`; returns the signature text (base64, as the manifest carries it). */
export function verifyArtifact(artifact, version, pubkeyBase64) {
  const sigPath = `${artifact}.sig`;
  if (!existsSync(artifact)) fail(`${artifact} is missing`);
  if (!existsSync(sigPath)) fail(`${sigPath} is missing (was the build run with VICEMAIL_UPDATER=1?)`);
  const signatureBase64 = readFileSync(sigPath, "utf8").trim();
  const lines = unwrap(signatureBase64, sigPath);
  if (lines.length < 4 || !lines[2].startsWith("trusted comment: ")) fail(`${sigPath} has no trusted comment`);
  const sig = Buffer.from(lines[1], "base64");
  const globalSig = Buffer.from(lines[3], "base64");
  const trusted = lines[2].slice("trusted comment: ".length);
  if (sig.length !== 74 || globalSig.length !== 64) fail(`${sigPath} has an unexpected length`);
  const { keyId, key } = publicKey(pubkeyBase64);
  if (!sig.subarray(2, 10).equals(keyId)) fail(`${sigPath} was made with another key than tauri.conf.json names`);
  const algorithm = sig.subarray(0, 2).toString();
  const data = readFileSync(artifact);
  const message = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  if (algorithm !== "ED" && algorithm !== "Ed") fail(`${sigPath}: unknown signature algorithm ${algorithm}`);
  if (!verify(null, message, key, sig.subarray(10))) fail(`${sigPath} does not match ${basename(artifact)}`);
  if (!verify(null, Buffer.concat([sig.subarray(10), Buffer.from(trusted, "utf8")]), key, globalSig)) {
    fail(`${sigPath}: the trusted comment is not signed`);
  }
  const signed = trusted.split("\t").find((field) => field.startsWith("version:"))?.slice("version:".length);
  if (signed !== version) fail(`${sigPath} is signed for version ${signed ?? "(none)"}, expected ${version}`);
  return signatureBase64;
}

function parseOptions(args) {
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const name = args[i];
    const value = args[i + 1];
    if (!name?.startsWith("--") || value === undefined) fail(`expected --option value pairs, got ${name ?? "nothing"}`);
    options[name.slice(2)] = value;
  }
  return options;
}

function manifest(options) {
  const { version, out } = options;
  const baseUrl = options["base-url"];
  if (!version || !SEMVER.test(version)) fail("--version must be semver");
  if (!baseUrl || !/^https:\/\/[^/]+\//.test(baseUrl)) fail("--base-url must be an https URL with a path");
  if (!out) fail("--out is required");
  const notes = options["notes-file"] ? readFileSync(options["notes-file"], "utf8").trim() : (options.notes ?? "");
  const pubDate = options["pub-date"] ?? new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
  if (Number.isNaN(Date.parse(pubDate))) fail("--pub-date is not a date");
  const platforms = {};
  for (const key of ["darwin-aarch64", "windows-x86_64"]) {
    const artifact = options[key];
    if (!artifact) continue;
    platforms[key] = {
      signature: verifyArtifact(artifact, version),
      url: `${baseUrl.replace(/\/$/, "")}/${encodeURIComponent(basename(artifact))}`,
    };
  }
  if (Object.keys(platforms).length === 0) fail("no artifact given (--darwin-aarch64, --windows-x86_64)");
  const document = { version, notes, pub_date: pubDate, platforms };
  writeFileSync(out, `${JSON.stringify(document, null, 2)}\n`);
  process.stdout.write(`updater-manifest: wrote ${out} (${Object.keys(platforms).join(", ")})\n`);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const [command, ...rest] = process.argv.slice(2);
  if (command === "verify") {
    const [artifact, version] = rest;
    if (!artifact || !version) fail("usage: verify <artifact> <version>");
    verifyArtifact(artifact, version);
    process.stdout.write(`updater-manifest: ${basename(artifact)} verified for ${version}\n`);
  } else if (command === "manifest") {
    manifest(parseOptions(rest));
  } else {
    process.stdout.write(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 19).map((l) => l.replace(/^\/\/ ?/, "")).join("\n") + "\n");
    process.exit(command === "-h" || command === "--help" ? 0 : 2);
  }
}
