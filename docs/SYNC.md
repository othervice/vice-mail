# Settings sync format

Status 23.11.2026. The logic (records, versions, merge rules, encryption, planning, the pass
over a transport) is implemented in `crates/mail-engine/src/settings_sync/`. The IMAP
transport, the `sync_record` table and the UI follow with the rest of stage 11; where this
document describes them, it says so ("planned").

The settings sync merges what only the app knows between devices. The mail server already
syncs mails, folders, flags and drafts; they are not part of this. Passwords, tokens and keys
never leave the device.

## Folder

- **Planned (IMAP):** one dedicated folder in an account the user picks (the "sync account").
  It is created but not subscribed, so most other mail clients do not show it.
- **One entry per record version.** On IMAP an entry is one message: two header values (below)
  and the age ciphertext as the body. Changing a record means appending the new version, then
  deleting the old one. A crash in between leaves two versions; the next pass of any device
  removes the loser.
- **Header values** are the only plaintext besides size and time:
  - *opaque id*: 32 lowercase hex digits for a record, `key-` plus 32 hex digits for the key
    record;
  - *version*: the hybrid logical clock value, `TTTTTTTTTTTTT-CCCCCCCC-<device uuid>`
    (13 decimal digits of Unix milliseconds, 8 hex digits of counter, the writing device's
    random id), for example `1764000000000-0000002a-6f9619ff-8b86-4d11-b42d-00c04fc964ff`.
    Fixed width, so the text sorts like the versions.
- The transport is a trait, `SettingsTransport` (list, read, write, delete). Tests use an
  in-memory folder; a self-hosted server or a plain directory could be further transports.

## Key record

- At setup a random age X25519 identity is generated: the **sync key**.
- The key record holds it, encrypted with the passphrase using age's scrypt recipient. The
  plaintext is JSON: `{"format": 1, "opaqueId": …, "version": …, "identity": "AGE-SECRET-KEY-1…"}`.
- **Work factor:** log2(N) = 18, so 256 MiB of memory per attempt. Measured on an Apple M4
  (release build): wrap 356 ms, unwrap 341 ms; log2(N) = 20 would take 1.4 s and 1 GiB. The key
  is unwrapped once per device (when joining, and after a passphrase change), so the delay is
  rare. An attacker who copies the folder pays the same memory and time for every guess. The
  value is fixed instead of age's per-machine calibration (about one second), so slow devices
  can still unwrap what fast ones wrote.
- **Upper bound when unwrapping:** log2(N) ≤ 20. A forged key record with a huge factor fails
  at once instead of making the device work for hours (age's own bound is "calibrated + 4",
  24 or more on fast machines).
- The key record id is `key-` + the first 16 bytes (hex) of
  `HMAC-SHA-256(id_key, "vice-mail settings-sync v1 key-record id")`. A device that holds the
  key can tell whether the folder still uses it without the passphrase. Several key records
  with the same id are the same key under different passphrases (an interrupted passphrase
  change); the next pass keeps the newest and deletes the others.

## Records

### Opaque id

`id_key = HMAC-SHA-256(key = sync key text "AGE-SECRET-KEY-1…", "vice-mail settings-sync v1 opaque-id key")`,
then `opaque id = hex(first 16 bytes of HMAC-SHA-256(id_key, kind || 0x00 || id))`.

The id is the same for every version of one record, so a device can group versions and skip
the ones it knows without decrypting. Without the key it reveals neither kind nor id. HMAC
comes from the RustCrypto crates `hmac` and `sha2`; there is no own cryptography.

### Encrypted payload

Each record is encrypted with age to the sync key's public key. The plaintext is JSON, padded
with spaces to a multiple of 256 bytes:

```json
{
  "format": 1,
  "opaqueId": "…32 hex digits…",
  "version": "1764000000000-00000000-…",
  "kind": "signature",
  "id": "8f1d…-…",
  "content": { "html": "<p>Best regards</p>" }
}
```

A tombstone has `"deleted": "<version it deletes>"` instead of `content`.

When reading, the payload is checked against the header: `opaqueId` and `version` must equal
the header values, and the opaque id recomputed from `kind` and `id` must equal the header's.
A mismatch (a server that swaps headers, replays an old record under a newer version, renames
it) or a failed authentication rejects the entry; it is reported, and the other entries sync as
usual. Rejected entries never count as a version. Damaged and misplaced ones are deleted once a
valid winner of their record is stored; entries of another key, of a newer app version
(`format` > 1 or an unknown kind) or with malformed content stay on the server. An entry
damaged under an unchanged header is only noticed by a device that downloads it: other
devices, or a periodic full check on the writing device (`PassMode::FullCheck`).

Content shapes are strict (unknown fields are refused). A change of shape therefore needs a
new `format` or a new kind name; older versions then leave such records alone.

### Kinds

Only kinds in the registry (`REGISTRY` in `record.rs`) exist; of the `setting` table only the
keys in `SYNCED_SETTINGS` (`ui.theme`, `ui.language`). Device settings such as `prefetch.*`
stay local.

| Kind (`kind`) | Id | Merge rule | Removed elsewhere | Max content |
|---|---|---|---|---|
| `setting` | `global/<key>` or `<account uuid>/<key>` | newer wins | applied | 4 KiB |
| `signature` | UUID | newer wins, edit beats delete | applied | 64 KiB |
| `template` | UUID | newer wins, edit beats delete | applied | 256 KiB |
| `rule` | UUID | newer wins, edit beats delete | applied | 16 KiB |
| `savedView` | UUID | newer wins, edit beats delete | applied | 16 KiB |
| `snooze` | `<account uuid>/<Message-ID>` | newer wins | applied | 1 KiB |
| `pin` | `<account uuid>/<Message-ID>` | newer wins | applied | 1 KiB |
| `account` | account UUID | per field | **user is asked** | 16 KiB |
| `identity` | UUID | newer wins, edit beats delete | applied | 16 KiB |
| `aiSettings` | UUID | newer wins, edit beats delete | applied | 16 KiB |
| `device` | device UUID | newer wins | applied | 4 KiB |

Mails are referred to by account UUID and Message-ID (without angle brackets), never by local
ids or UIDs.

### No secrets

Content is only accepted through a validating type:

- `Secret<T>` has no `Serialize`, so nothing holding a secret can become content.
- No object key may name a password, passphrase, secret, token, API or private key,
  credential, authorization or cookie (case, `_` and `-` ignored; `maxTokens` is fine).
- No string may contain key material (`AGE-SECRET-KEY-`, `PRIVATE KEY-----`).
- Account records only have the fields `email`, `displayName`, `color`, `imap`, `smtp`, and the
  server entries only `host`, `port`, `security`, `username`. `writes_enabled` is deliberately
  not synced: write access is switched on by the user per account and device, and an account
  taken over from another device starts read-only.

## Versions

A hybrid logical clock (Kulkarni et al. 2014): (wall time in ms, counter, device id), ordered in
this sequence. A local change takes `max(last, wall clock)` and increments the counter on
equal time; every version read from the folder is observed with the usual receive rule, so a
later change is always newer than everything the device has seen. The clock state is restored
from the highest version after a restart.

**Drift bound:** a version more than 5 minutes ahead of the local wall clock is deferred: its
record is not merged, not deleted and not overwritten until the local clock is within 5 minutes.
Without the bound, one device with a clock in 2099 would win every conflict and drag every
other clock with it. NTP keeps desktops within well under a second; 5 minutes also covers a
clock that has not resynced after sleep.

## Merge rules

The winner depends only on the set of versions, not on their order; keeping only the winner and
merging it with later versions gives the same result as merging everything (both checked by
tests).

- **Newer wins** (settings, snooze, pin, device): the highest version, a tombstone like any
  other.
- **Edit beats delete** (signatures, templates, rules, views, identities, AI settings): a
  tombstone carries the version its device had seen and removes the versions up to that one.
  A version above it was not seen by the deleting device, so it was edited concurrently (or
  re-created later) and beats the delete, even if the delete is later in time. Among surviving
  versions the newest wins; if none survives, the tombstone with the highest deleted version.
- **Per field** (accounts): as above per record; among several surviving versions each field
  takes its newest value. If the result equals an existing version, that one wins; otherwise
  it is stored as a new version.
- **Account removed elsewhere:** a winning tombstone does not delete a live local account. The
  user is asked: remove it here too, or keep it, which stores it again under a new version and
  brings it back on every device.

## A pass

1. List the folder (headers only). Without any key record: "not set up". With key records of
   another key only: "reset elsewhere, enter the passphrase again". Nothing is written then.
2. Decide what to download: not the versions equal to the local or last synced version, and not
   those that provably lose against the local version from the header alone.
3. Download, decrypt, check against the header.
4. Merge every record. Upload the winner if it is not on the server, then delete the other
   versions and extra copies (upload first, so a crash leaves a duplicate, never a gap), then
   store the result locally with its synced version.

A failed upload leaves the record unchanged locally and is retried by the next pass. Reading
and sending mail do not depend on the sync account.

## Tombstones and absent records

- A deletion is a tombstone record. It stays on the server for **180 days** after the
  deletion, then the next pass of any device deletes it. Locally the tombstone stays (a few
  dozen bytes), so an old version that comes back later is recognised as deleted.
- If such an old version comes back after the server's tombstone has expired (a replay, or a
  device that was away for longer), the deletion is announced again with a fresh tombstone for
  another 180 days. A device that has never seen the record cannot tell a replay from a record
  and takes it over until that announcement arrives.
- A local record that was synced before and is no longer on the server was either lost
  (deleted by another client, data loss) or deleted with a tombstone that has expired since. It
  counts as deleted only if its version **and** this device's last complete pass are older than
  180 days; otherwise a tombstone would still be there. Such a stale record is forgotten
  locally (accounts: the user is asked). Everything else is uploaded again, so the folder heals.
- A device that was away for longer than 180 days therefore drops its stale records instead of
  bringing deleted ones back; its changes newer than 180 days are uploaded and follow the merge
  rules (an edit the deleting device never saw beats the delete).

## Passphrase

- **Setup:** new key, key record with the passphrase, then the first pass uploads everything.
- **Join (second device):** the key records are tried newest first. A wrong passphrase is a
  typed error (`WrongPassphrase`); nothing is written on the server or locally.
- **Change:** the key is wrapped again with the new passphrase and written as a new key record,
  then the old one is deleted. Other devices keep working with the key they hold.
- **Forgotten / reset:** every entry in the folder is deleted, a new key is set up, and every
  local record counts as not synced, so the next pass uploads this device's data. Other devices
  get "reset elsewhere", join with the new passphrase and merge their data like a new device.

## What the mail server can see

- **Number of entries**, and that one of them is a key record.
- **Sizes**: plaintexts are padded to 256-byte blocks, and age adds a random "grease" stanza of
  up to a few hundred bytes to every header.
- **Times**: when an entry was stored (IMAP internal date) and the version's time.
- **Which entries are versions of the same record** (same opaque id), so how often and when a
  record changes, and how many records there are.
- **The writing device's random id** in every version: how many devices take part and which
  device wrote what when. The id is random and not tied to the hardware.

It does not see kinds, ids, contents, account addresses or which kind a record is. It can
delete, withhold or replay entries; deletions heal from the devices' local copies, replays are
recognised (see above), and modifications are detected and rejected.

## Measurements

1,000 records (2 settings, 300 signatures of about 1 KiB, 300 snoozes, 200 pins, 150 rules, 48
views; 912 KiB encrypted), in-memory folder, age 0.11.5, Apple M4
(`cargo test -p mail-engine settings_sync::tests::perf -- --nocapture`):

| Step | Debug build (tests) |
|---|---|
| encrypt 1,000 | 519 ms |
| decrypt and check 1,000 | 497 ms |
| plan for a new device (without crypto) | 5.5 ms |
| first pass, uploading device | 533 ms |
| first pass, second device (download, decrypt, plan, apply) | 519 ms |
| repeated pass without changes (nothing downloaded) | 14 ms |
| full check (everything downloaded and checked) | 526 ms |

In the debug build the dependencies are unoptimised. With optimisation (release build of the
same age calls) encrypting 1,000 records of 1,280 bytes takes 41 ms and decrypting 39 ms;
scrypt at log2(N) = 18 takes 356 ms to wrap and 341 ms to unwrap (debug build: 12.8 s each).
The IMAP round trips of the planned transport come on top.
