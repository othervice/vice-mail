# Settings sync format

Status 23.11.2026. The logic (records, versions, merge rules, encryption, planning, the pass
over a transport) is in `crates/mail-engine/src/settings_sync/`; the integration (change
capture in the store, the IMAP folder, the flows in the core) is described in decision 0022.
The settings page itself (UI) follows.

The settings sync merges what only the app knows between devices. The mail server already
syncs mails, folders, flags and drafts; they are not part of this. Passwords, tokens and keys
never leave the device.

## Folder

- **One folder** in an account the user picks (the "sync account"): `Settings-Sync` at the top
  level, or `INBOX<delimiter>Settings-Sync` on servers that keep every folder below the inbox.
  The name is plain ASCII without hierarchy delimiters, spaces or a leading dot (no modified
  UTF-7, never split into a hierarchy, valid on every common server), neutral (no product
  name) and tells a person who finds it in another mail client what it is.
- **Not subscribed**: created with CREATE and unsubscribed right after (some servers subscribe
  new folders themselves), so most mail clients do not show it. The app's own mail sync leaves
  it out of the folder list; seeing it there tells a new device that the account holds a sync.
- **Writing only there.** Turning the sync on allows the app to write to this folder only
  (decision 0014): its connection is read-only for the rest of the account, also when writing
  is off for the account. The IMAP adapter checks every command line before it is sent:
  `SELECT`, `CREATE`, `UNSUBSCRIBE` and `APPEND` name exactly this folder, and
  `UID STORE <uids> +FLAGS.SILENT (\Deleted)`, `UID EXPUNGE <uids>` and `EXPUNGE` pass only
  while it is the selected mailbox. Everything else fails with `read_only`.
- **On a JMAP account** (stage 14, decision 0041, addendum 2) the folder is a mailbox of the same
  name, created without a role. An entry is uploaded and imported there (`Email/import`); the
  listing reads the size and the two header fields (`Email/query`, `Email/get`); a body is the
  downloaded message without its header, read up to 4 MiB at most. The connection is read-only
  like over IMAP; its own writes are checked before they are sent: `Mailbox/set` only creates this
  mailbox, `Email/import` only into it, `Email/set` only destroys messages listed in it (or takes
  one that lies in other mailboxes too out of this one). Everything else fails with `read_only`.
  The limits of a pass (below) are the same.
- **One message per record version**: two header fields and the age ciphertext as the body.
  Changing a record means appending the new version, then deleting the old one. A crash in
  between leaves two versions; the next pass of any device removes the loser.
- **Header fields** are the only plaintext besides size and time:
  - `X-Settings-Sync-Id`, the *opaque id*: 32 lowercase hex digits for a record, `key-` plus 32
    hex digits for the key record;
  - `X-Settings-Sync-Version`, the *version*: the hybrid logical clock value,
    `TTTTTTTTTTTTT-CCCCCCCC-<device uuid>` (13 decimal digits of Unix milliseconds, 8 hex
    digits of counter, the writing device's random id), for example
    `1764000000000-0000002a-6f9619ff-8b86-4d11-b42d-00c04fc964ff`. Fixed width, so the text
    sorts like the versions.
- **The rest of the message**: `Date` (the time of writing), `Subject: Settings sync data
  (encrypted, do not edit)`, `Content-Type: text/plain; charset=us-ascii`, and the body in
  age's ASCII armor (`-----BEGIN AGE ENCRYPTED FILE-----`, 7-bit, CRLF). Appended with `\Seen`.
  Messages without both header fields are not the sync's: it never reads, changes or deletes
  them (also not when it is reset).
- **Changes are noticed by the folder's state.** At the start of every pass the folder is
  selected; UIDVALIDITY, UIDNEXT, EXISTS and HIGHESTMODSEQ are kept. Unchanged, with nothing
  local to upload, the pass ends there (one SELECT on a kept connection). Passes run after a
  local change (two seconds later, so a burst is one pass), after each pass of the sync
  account's mail sync, and at the mail sync's poll interval.
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
devices, or a periodic full check on the writing device (`PassMode::FullCheck`). Entries listed
too large to be a record are not downloaded at all and stay like those of another key (see "A
pass").

Content shapes are strict (unknown fields are refused). A change of shape therefore needs a
new `format` or a new kind name; older versions then leave such records alone.

### Kinds

Only kinds in the registry (`REGISTRY` in `record.rs`) exist; of the `setting` table only the
keys in `SYNCED_SETTINGS`: `ui.theme`, `ui.language`, `reading.mark_read_on_open`,
`compose.send_delay_seconds`, `notifications.settings`, and since stage 8 (decision 0025)
`compose.default_quote_position` (where new replies put the answer), `ui.shortcuts.macos`, `ui.shortcuts.windows` (the shortcut changes of one platform, a list of
`{ command, shortcuts: [{ key, shift?, mod? }] }` ordered by command) and `ui.text_size`
(80 to 150), since stage 16 (decision 0033) `compose.archive_on_reply` and
`compose.read_receipts`, and (decision 0036) `ui.accent_theme` (one of `teal`, `blue`, `violet`,
`rose`, `orange`, `green`; default `orange`, decision 0038; a value a device does not know reads
as the default), (decision 0039) `ui.conversations` (a boolean, default on) and since stage 17
`ui.column_views` (the views of the column's "Views" section and of the rail in the user's order,
a list of `{ id, shown }`; `[]` is the default, a view a list does not name keeps its default place,
and entries of views a device does not know are kept). Device settings such
as `prefetch.*` (what this machine keeps offline), `inbox.settings` (categories and bundles on
this screen), `ui.recent_commands` (the palette's history), `ui.layout.<slot>` (window layouts)
and `ui.unread_first` (the views that group unread mail on top) stay local.

| Kind (`kind`) | Id | Merge rule | Removed elsewhere | Max content | Local data |
|---|---|---|---|---|---|
| `setting` | `global/<key>` or `<account uuid>/<key>` | newer wins | applied | 16 KiB (4 KiB before stage 8) | `setting` |
| `signature` | signature UUID (a stage 5 text signature kept its identity's UUID) | newer wins, edit beats delete | applied | 256 KiB | `signature` |
| `template` | UUID | newer wins, edit beats delete | applied | 256 KiB | `template` |
| `rule` | UUID | newer wins, edit beats delete | applied | 16 KiB | `rule` |
| `savedView` | UUID | newer wins, edit beats delete | applied | 16 KiB | `saved_view` |
| `snooze` | `<account uuid>/<Message-ID>` | newer wins | applied | 1 KiB | `snooze` |
| `pin` | `<account uuid>/<Message-ID>` | newer wins | applied | 1 KiB | `pin` |
| `account` | account UUID | per field | **user is asked** | 16 KiB | `account` |
| `identity` | UUID | newer wins, edit beats delete | applied | 16 KiB | `identity` |
| `aiSettings` | UUID | newer wins, edit beats delete | applied | 16 KiB | none yet |
| `device` | device UUID | newer wins | applied | 4 KiB | `sync_device` |
| `task` | `<account uuid>/<Message-ID>` | newer wins | applied | 1 KiB | `task` |
| `senderRule` | `address:<v>`, `domain:<v>` or `list_id:<v>` | newer wins | applied | 1 KiB | `sender_rule` |
| `accountGroup` | UUID | newer wins, edit beats delete | applied | 16 KiB | `account_group` |
| `remoteContent` | `<account uuid>/<address>` | newer wins | applied | 1 KiB | `remote_content_sender` |
| `contact` | normalised address | newer wins | applied | 4 KiB | `contact.user_name`, `contact.contact_group` |
| `readingList` | `<account uuid>/<Message-ID>` | newer wins | applied | 1 KiB | `reading_list` (stage 16, decision 0033) |
| `mailboxPref` | `<account uuid>/<decoded mailbox path>` | newer wins | applied | 1 KiB | `mailbox_pref` (stage 16, decision 0033) |
| `blockedSender` | normalised address | newer wins | applied | 1 KiB | `blocked_sender` (stage 16, decision 0033) |
| `attachmentPin` | `<account uuid>/<Message-ID>/<attachment index>` | newer wins | applied | 2 KiB | `attachment_pin` (stage 16, decision 0036) |

Mails are referred to by account UUID and Message-ID (without angle brackets), never by local
ids or UIDs. A pinned attachment adds the attachment's index in the reading view as the last
segment (a decimal number without leading zeros; the Message-ID itself may contain `/`). Sender rules are referred to by their normalised key, not by a UUID: two devices
that set "always as newsletter" for the same sender change the same record.

**Content per kind** (JSON, camelCase, unknown fields refused):

| Kind | Content |
|---|---|
| `setting` | the setting's JSON value (not null) |
| `signature` | `{name?, text?, html?, images?}`, text or html at least (stage 10: a signature of the pool; `images` are `[{id, contentId, fileName, mime, dataBase64}]`, PNG/JPEG/GIF/WebP, at most 8 and 128 KiB; a stage 11 record `{text}` still reads) |
| `identity` | `{account, email, name, replyTo, isDefault, signatureNew?, signatureReply?}` (signatures are records of their own; the identity names its defaults by signature UUID, absent when none is set; a name that points nowhere means none) |
| `template` | `{name, subject, html, to?, cc?, bcc?}` (stage 10; stage 16: the recipients it prefills as `[{name, email}]`, each field left out when empty, at most 100 valid addresses together; a stage 10 record still reads) |
| `rule` | `{name, position, enabled, definition: {matchAll, conditions, actions, stop}}` |
| `snooze` | `{untilMs}`; whether it came back is each device's own (every device's timer brings it back) |
| `pin` | `{pinnedAtMs}` |
| `task` | `{doneAtMs}` (null while open) |
| `senderRule` | `{category, important}`: `person`, `notification`, `newsletter` or null; not both empty |
| `accountGroup` | `{name, position, accounts: [account uuid, …]}` |
| `remoteContent` | `{}` (that it exists is all) |
| `readingList` | `{addedAtMs}` |
| `mailboxPref` | `{favoritePosition?, color?}`, at least one (colour index 0 to 7 of the account palette) |
| `blockedSender` | `{}` (that it exists is all) |
| `attachmentPin` | `{pinnedAtMs, name, mimeType, size}`: the sanitised file name (not empty, no controls), the declared type and the size when it was pinned, so the list shows it without the body |
| `contact` | `{userName?, mergedInto?}`, at least one: the name the user gave, and for an address the user merged into another contact that contact's normalised address (stage 10). Applied when this device has the address (and the other one); until then the record waits in `sync_record` and applies when the address appears in the directory. Dismissed duplicate suggestions stay on the device |
| `account` | per field `{version, value}`: `email`, `displayName`, `color`, `imap`, `smtp` |
| `device` | `{name, system, lastSeenMs}` |
| `savedView` | `{name, query, position}`: a saved search of the sidebar (stage 7, decision 0026), checked like a local save (name 1 to 100 characters, query 1 to 2048 bytes, both trimmed, no control characters). Names are unique per device only where they are typed; records from other devices are taken over even with a name in use. Recent searches stay on the device |
| `aiSettings` | any object (no local data yet; kept as they are) |

### No secrets

Content is only accepted through a validating type:

- `Secret<T>` has no `Serialize`, so nothing holding a secret can become content.
- No object key may name a password, passphrase, secret, token, API or private key,
  credential, authorization or cookie (case, `_` and `-` ignored; `maxTokens` is fine).
- No string may contain key material (`AGE-SECRET-KEY-`, `PRIVATE KEY-----`).
- Account records only have the fields `email`, `displayName`, `color`, `imap`, `smtp`, and the
  server entries only `host`, `port`, `security`, `username`. `writes_enabled` is deliberately
  not synced: write access is switched on by the user per account and device, and an account
  taken over from another device starts read-only. Other servers for an existing account wait
  for the user (see "Accounts taken over" below).

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
  brings it back on every device. Until the user decides, the removals of the account's other
  records (identities, signatures, snoozes, pins, tasks, remote content senders, its settings)
  wait too; keeping the account renews them as well, so they come back with it.

## A pass

1. List the folder (headers only). Without any key record: "not set up". With key records of
   another key only: "reset elsewhere, enter the passphrase again". Nothing is written then.
2. Decide what to download: not the versions equal to the local or last synced version, and not
   those that provably lose against the local version from the header alone. Once a day the
   pass downloads and checks every entry (`PassMode::FullCheck`). **Limits** (security review
   F2, `FetchLimits`), decided from the listed sizes (`RFC822.SIZE`) before anything is read:
   - An entry listed larger than **2 MiB** is never downloaded: no record can be that large (the
     largest ciphertext a device opens is 1 MiB, about 1.4 MiB in ASCII armor; the largest
     content is 256 KiB). It never counts as a version and is never deleted, neither as a loser
     nor as a damaged or misplaced entry; like a rejected entry it is reported and counted in
     the status. Joining reads no key record listed that large, and neither a pass nor a
     passphrase change keeps or deletes one as a copy of the key record. Only a reset deletes it.
   - A pass downloads at most **5,000 entries and 64 MiB**. Records are taken in the order of
     their opaque ids, each with all of its new entries or not at all. A record that does not
     fit waits as a whole (nothing of it is uploaded, deleted or changed locally), is reported
     and counted in the status; later records that fit still come. The next check then runs a
     pass even if the folder did not change, and a pass that postponed a record does not count
     as complete (see "Tombstones and absent records"). Records downloaded once are known in
     the next pass, so repeated passes work through the folder, as long as entries that stay
     unreadable (another key, a newer app version) do not fill a pass on their own.
   - A full check downloads the new entries first; the known entries it only re-checks follow
     as far as the limits leave room, the others are treated as in a normal pass.
3. Download, decrypt, check against the header.
4. Merge every record. Upload the winner if it is not on the server, then delete the other
   versions and extra copies (upload first, so a crash leaves a duplicate, never a gap), then
   store the result locally with its synced version.

A failed upload leaves the record unchanged locally and is retried by the next pass. A record
that changed locally while the pass ran is not overwritten; the next pass merges both. Reading
and sending mail do not depend on the sync account: the sync has a connection of its own, and
its failure only shows in its status.

**Local side.** Every change of a synchronised table gets its new version in the transaction
that makes it: triggers mark the record, and before the transaction commits the store hands
the new content to the core, which versions it (unchanged content keeps its version). Records
of a pass are written into their tables without such a mark, so applying never creates a new
version.

**The same thing on both sides under different ids.** The sync account is set up on the second
device before it joins, with its own account id and default identity. A local account with the
address of an account in the folder, or a local identity with the account and address of one
in the folder, that was never synced takes the folder's id (the keychain entry, the stored
mail and the local data stay); its local records are replaced by the folder's.

**Accounts taken over** start read-only and without a password ("password missing"), and
without a default identity of their own (their identities come from the folder). Their keychain
entry gets a fresh random name (`synced:` and 32 hex digits) instead of the account UUID, so a
password this device already holds under that UUID is never used for servers named only by the
folder (decision 0040).

**Server settings are a credential boundary** (decision 0040, security review F1). A record that
names other servers (any difference in host, port, security or user name of IMAP or SMTP) for an
account that exists here is applied without them: email, name and colour are written, the
servers stay, and the folder's servers wait in `account_server_change` until the user decides on
the account's page. Confirming removes the stored password first, switches writing off, then
writes the new servers; the account asks for its password again. Dismissing keeps this device's
servers and is remembered, so the same change is not offered again (a different one is). While a
change waits or was dismissed, the sync sees the folder's servers as the account's, so a local
change of another field never sends this device's servers back. This also applies to a local
account that takes the folder's id when a device joins.

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
  counts as deleted only if its version **and** this device's last complete pass (one that
  postponed no record) are older than 180 days; otherwise a tombstone would still be there. Such a stale record is forgotten
  locally (accounts: the user is asked). Everything else is uploaded again, so the folder heals.
- A device that was away for longer than 180 days therefore drops its stale records instead of
  bringing deleted ones back; its changes newer than 180 days are uploaded and follow the merge
  rules (an edit the deleting device never saw beats the delete).

## Passphrase

- **Setup:** new key, key record with the passphrase, then the first pass uploads everything.
- **Join (second device):** the key records are tried newest first. A wrong passphrase is a
  typed error (`WrongPassphrase`); nothing is written on the server or locally. The device
  first shows what joining would take over (a pass planned against its data, nothing
  written), then merges like a new device.
- **Passphrase rules:** at least 8 characters; the UI shows a hint about strength and what
  forgetting it means. The keychain holds the sync key, never the passphrase (decision 0014).
- **Change:** the key is wrapped again with the new passphrase and written as a new key record,
  then the old one is deleted. Other devices keep working with the key they hold.
- **Forgotten / reset:** every entry in the folder is deleted, a new key is set up, and every
  local record counts as not synced, so the next pass uploads this device's data. Other devices
  get "reset elsewhere", join with the new passphrase and merge their data like a new device.

## What the mail server can see

- **Number of entries**, and that one of them is a key record.
- **Sizes**: plaintexts are padded to 256-byte blocks, and age adds a random "grease" stanza of
  up to a few hundred bytes to every header.
- **Times**: when an entry was stored (IMAP internal date, the `Date` field) and the version's
  time; when a device checks the folder (its connection and SELECTs).
- **That the account holds a settings sync** (the folder and its fixed subject).
- **Which entries are versions of the same record** (same opaque id), so how often and when a
  record changes, and how many records there are.
- **The writing device's random id** in every version: how many devices take part and which
  device wrote what when. The id is random and not tied to the hardware.

It does not see kinds, ids, contents, account addresses or which kind a record is. It can
delete, withhold or replay entries; deletions heal from the devices' local copies, replays are
recognised (see above), and modifications are detected and rejected. Entries it adds are read
only within the limits of a pass (see "A pass"); one listed larger than 2 MiB is never read. A
server that lists a smaller size than it sends is bounded only by the IMAP library (security
model, N-1).

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

**Over IMAP** (two engines with their own databases, local Stalwart 1.1.1 over TLS, debug
build, scrypt log2(N) = 10 in the tests; `cargo test -p mail-engine --test settings_sync --
--include-ignored a_thousand --nocapture`): 1,000 sender rules plus the account, identity and
device records (1,003 entries):

| Step | Time |
|---|---|
| creating the 1,000 rules (each a write transaction with its capture) | 2.3 s |
| turning the sync on: key, first pass, 1,003 APPENDs | 0.76 s |
| joining on the second device: preview (download, decrypt, plan), then the first pass (download again, match the sync account and its identity, merge, write 1,003 records into the tables) | 1.9 s |
| a pass without changes (forced: list the headers, nothing downloaded) | 37 ms |
| the daily full check (download and check everything) | 0.47 s |
