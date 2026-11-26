# Architecture

As of 2026-11-22. The versions are the stable ones current on that day. Stage 0 tested the risky
assumptions with throwaway code; measurements and reasons are in `docs/decisions/0001` to `0005`.

## Overview

```
React UI (WebView)
   │   typed commands and events, the only way
app/src-tauri            thin shell: windows, commands, wiring
   │
mail-engine              accounts, sync planning, outbox, threads, categories, rules
   ├── mail-store        SQLite, migrations, FTS5, queries
   ├── mail-imap         IMAP and SMTP adapter, implements MailBackend
   ├── mail-jmap         JMAP adapter (stage 14, 0041), implements MailBackend, MailWriter, MailSender
   ├── mail-content      parsing, HTML sanitising, link checking, file names (pure, no IO)
   ├── webview2-guard    Windows only: intercept navigations in the mail frame (only crate with `unsafe`, 0011)
   └── mail-mcp          MCP server: helper program and tools against `MailAccess` (0015)
   └── mail-model        types, errors, capabilities, MailBackend trait, no IO
```

Later the MCP server (stage 12), `mail-ai` (stage 13, optional) and, if needed, further adapters (stage 14) are added. The
settings sync (stage 11) starts as a module in `mail-engine` and only becomes a crate of its own
once it is large enough.

## Stack

| Area | Choice | Version | Why |
|---|---|---|---|
| Desktop shell | Tauri | 2.12 (crate 2.12.7, CLI 2.12.6) | system WebView instead of a bundled browser: small app, little memory, Rust core; spike app: `.dmg` 2.2 MB, Windows installer 1.9 MB (0004) |
| Core | Rust stable, edition 2024, Tokio | at least 1.98, tested 1.100 | speed, memory safety; mail-parser requires 1.98, Tauri 2.12 requires 1.90 (0006) |
| UI | React, TypeScript strict | 19.3.1 / 7.0.2 | largest ecosystem for the building blocks needed. Typecheck with TS 7 (package alias `typescript7`); `typescript` 6.0.3 only for typescript-eslint, which needs the classic TS API |
| Build | Vite, pnpm | 8.3 / 12.19 | fast, standard with Tauri |
| Styling | Tailwind CSS with own tokens | 4.3.5 | tokens as CSS variables, no runtime CSS |
| Building blocks | Radix UI (`radix-ui`), Lucide icons (`lucide-react`), cmdk | 1.8.1 / 1.77 / 1.1 | accessible primitives without an imposed look. Beware CSP: Radix Dialog, Popover, Select and ScrollArea insert `<style>` elements; check before using them in the release build |
| List | TanStack Virtual (`@tanstack/react-virtual`) | 3.14.26 | virtualisation for very long lists; with 10,000 entries at most about 40 rows in the DOM. List as `listbox` with `aria-activedescendant` (stage 2). Stage 8: the quick actions in the rows are pure mouse targets without role or focus (an `option` must not contain controls, axe `nested-interactive`); keyboard and screen reader have the same actions as shortcuts, in the toolbar and in the command palette. Decision 0031: bundles, "Show all" and "Back to inbox" are `option`s themselves (arrow keys, Enter, Space; axe `aria-required-children`), and every `option` is named by its content instead of by `aria-label` (WCAG 2.5.3) |
| Data in the frontend | TanStack Query, Zustand | 5.108.2 / 5.1.0 | Query for data from the core, Zustand for pure UI state |
| Editor | TipTap | 3.35 | mature rich-text editor with controllable HTML |
| Languages | i18next, react-i18next | 27.2 / 18.0 | standard, English and German; text literals in components are forbidden by `eslint-plugin-i18next` (`no-literal-string`) |
| Lint | ESLint, typescript-eslint, eslint-plugin-react-hooks, eslint-plugin-i18next | 10.15 / 8.73 / 7.1 / 6.1 | `eslint-plugin-react` is ruled out (no release since April 2025, no ESLint 10) |
| IDs | uuid | 1.29 | account IDs (UUID v4) |
| Database | SQLite via rusqlite (bundled, FTS5, tokenizers `unicode61 remove_diacritics 2` and, from stage 22, `trigram remove_diacritics 1` (0074)) | 0.41.1 (SQLite 3.54.1, 0006) | one process, one writer: synchronous rusqlite is simpler and faster than SQLx; with 500,000 mails a list page takes 0.04 ms, first search hits under 0.2 ms (0003) |
| Migrations | rusqlite_migration | 2.7.0 | small, uses `user_version`; pins rusqlite to ^0.41 (0006) |
| SQL functions | rusqlite feature `functions` (stage 19, 0061) | with rusqlite | the sort keys of a location are computed in Rust (`mail_model::sort_text`) and called from the statements that insert locations and from migration 0026; registered on the write connection only |
| IMAP | async-imap (Tokio, rustls) plus our own thin command layer in `mail-imap` | 0.12.0 | in continuous use at Delta Chat; connection, parser, IDLE come from the library. Status checking, response codes (APPENDUID, COPYUID, MODIFIED), ENABLE, QRESYNC, LIST-STATUS and modified UTF-7 via our own layer. 10,000 headers in 0.044 s over TLS (0002) |
| JMAP | our own adapter `mail-jmap` over the workspace's reqwest (HTTP/1.1, rustls from `mail_imap::tls`) and serde_json | – | stage 14 (0041): RFC 8620/8621 are JSON over HTTPS; the few methods (session, `Mailbox/get`, `Email/query`, `/changes`, `/get`, `/set`, `/import`, `EmailSubmission/set`, blobs, EventSource) are slimmer than a library. No new dependency |
| SMTP | lettre (protocol layer only, `smtp-transport` + `tokio1`) | 0.11.24 | the adapter does TLS, timeouts and STARTTLS itself, as with IMAP (platform verifier); lettre without its own TLS |
| Reading / writing MIME | mail-parser / mail-builder | 1.0.1 / 2.0.1 | robust against broken mails, from the Stalwart project |
| Body storage | zstd (stage 3, with checksum) | 0.14.4 | same ratio as Deflate, 4 to 20 times faster at compressing; files instead of BLOBs, so that the database and migration backups stay small (0009) |
| HTML sanitising | ammonia, html5ever, cssparser, url, icu_normalizer/icu_properties | 4.2.5 / 0.40 / 0.38 / 2.5 / 2.4 | allowlist sanitizer; CSS is rewritten token by token (no regex); Punycode decoded by ourselves per RFC 3492, deliberately not `idna` directly (no release since August 2025) |
| TLS | rustls with provider `aws-lc-rs`, tokio-rustls, verification via the system store with rustls-platform-verifier | 0.23 / 0.26 / 0.7, aws-lc-rs 1.19 | no OpenSSL; the default provider of rustls, released regularly, also builds for Windows without CMake and NASM (pre-assembled objects from the crate, 0002). Provider and client configuration are created in one place (`mail_imap::tls`), also for reqwest and the tests' TLS servers. Self-signed server certificates work if the system trusts them. Additional root CA (`new_with_extra_roots`) only for tests against the test server |
| Opening in the system, file dialog | open (`shellexecute-on-windows`), rfd | 5.4.5 / 0.18.0 | open links and attachments in the system, save dialog; both only in Rust, testable behind traits. Not `tauri-plugin-dialog` (injects a script and brings `tauri-plugin-fs` along) |
| Attachment preview | image (`png`, `jpeg`, `gif`, `webp`), hayro | 0.25.10 / 0.9.0 | stage 16 (0036): images decoded and re-encoded, PDF pages rasterised, both in Rust; the WebView only sees PNG/JPEG generated by ourselves. The same decoders re-encode the images in mails (security review C-4, 0046; animated ones as GIF with the `gif` encoder of `image`), likewise in the helper, and since stage 17 the pictures the user chooses for contacts (0050). hayro is pure Rust (`hayro` itself `forbid(unsafe_code)`, `hayro-syntax` with one `unsafe` block for the page list, 0043), without a script engine and network, with embedded standard fonts and CMaps, but without a limit of its own for decompressed streams and time; hence the preview runs in the helper `mail-mcp` with a time and memory limit (C-2, 0043). Both Apache-2.0 OR MIT |
| Notifications, single instance | tauri-plugin-notification, tauri-plugin-single-instance (Windows only) | 2.8.1 / 2.5.5 | stage 9 (0020): native notifications only from Rust (no capability for the UI), clicks via the plugin's action handler; on Windows a second launch (`mailto:` link) passes its arguments on to the running app; since stage 21 (0069) a second launch without arguments opens the main window again, also after it was closed into the background. Since stage 18 (0051) new mail shows in the app's own popup window by default; failed sends and returned snoozes stay native. Both Apache-2.0 OR MIT |
| Popup window and sound (stage 18, 0051) | objc2, objc2-app-kit, objc2-foundation, objc2-quartz-core (macOS); windows, windows-registry (Windows) | 0.6.5 / 0.3.2 / 0.3.2 / 0.3.2; 0.62.2 / 0.6.1 | the versions Tauri, wry and hickory already build with, now direct dependencies of `mail-app`, safe calls only: on macOS the new-mail popup's `NSWindow` (never key, rounded content layer, not in the Window menu, every Space) and its sound (`NSSound`, `NSBeep`); on Windows the toast sound's file from the sound scheme (registry) played with WinRT's `MediaPlayer`. Before, objc2 was only in the desktop-test build. objc2 MIT, objc2-app-kit and objc2-quartz-core Zlib OR Apache-2.0 OR MIT, objc2-foundation MIT, windows and windows-registry MIT OR Apache-2.0 |
| Tray icon, running in the background (stage 21, 0069, 0070) | Tauri feature `tray-icon` (crate tray-icon), objc2-app-kit feature `NSDockTile`; own login items via windows-registry (Run key) and a launch agent file | 0.25.1 / 0.3.2 / 0.6.1 | the icon in the menu bar (macOS, a template image) or the notification area (Windows) with the unread count and a menu; tray-icon is Tauri's own crate (also builds muda), released 2026-09-16, MIT OR Apache-2.0, already in the lock file as Tauri's optional dependency. The Dock badge is set on `NSDockTile` (safe binding) so it stays current without a window. Not `tauri-plugin-autostart` (2.7.0, Apache-2.0 OR MIT): it brings `auto-launch` 0.6 with `smappservice-rs` and `os_info`, offers its commands to the UI, and its macOS main-app login item cannot pass `--background`; `objc2-service-management` (for `SMAppService`) had no release in the last 12 months (0.3.2 of 2025-10-04). The Run key and the plist are ~150 lines in `autostart.rs`, tested with a fake |
| Unread count on Windows, drawn from a font (stage 22, 0080) | skrifa; font Selawik Semibold (subset "Badge Digits", `app/src-tauri/fonts`) | 0.49.0 / 1.01 | the digits of the taskbar overlay and the notification-area badge are real glyphs, auto-hinted by skrifa (Google fontations) at exactly the pixel size Windows shows and filled by a small scanline rasteriser in `badge.rs`. skrifa was already linked through hayro at this version and with these features (released 2026-10-21, MIT OR Apache-2.0), so nothing new is compiled. Selawik is Microsoft's open-source stand-in for Segoe UI under the SIL Open Font License 1.1, which allows bundling and embedding with any software; the subset (3.6 kB, digits and the capitals the hinter measures) is renamed as the OFL asks and listed as a `font` entry by `scripts/licenses.mjs`. Not ab_glyph_rasterizer (in the tree too, last release August 2025) and not Segoe UI from the system (not testable on the Mac, different per Windows version) |
| In-app updates | tauri-plugin-updater | =2.14.1 | stage 15 (0037): manifest, signature verification (minisign/Ed25519, `requireSignedVersion`), replacing the app bundle (macOS) and NSIS installer in `passive` mode (Windows); only from Rust, no capability for the UI. Without default features (no `ring`, no system proxy, no zip), TLS via our reqwest/rustls through `configure_client`, hence pinned exactly. Released 2026-10-28, Apache-2.0 OR MIT. `tauri-plugin-process` not needed (`AppHandle::request_restart`) |
| WebView2 access | webview2-com (only in `webview2-guard`) | =0.39.1 | subscribe to `FrameNavigationStarting`, which wry does not offer (0011) |
| Keychain | keyring-core with apple-native-keyring-store and windows-native-keyring-store | 1.1 / 1.0 / 1.1 | macOS keychain (login keychain) and Windows credentials; recommended for apps by the `keyring` project, without a global default store and without Linux D-Bus. Service name = bundle ID, account name = account UUID; accounts adopted from the settings sync get a random name `synced:<hex>` (0040) |
| Account discovery | hickory-resolver, reqwest (rustls, without default features), roxmltree | 0.26 / 0.13 / 0.21 | DNS SRV, HTTPS for the domain's own autoconfiguration, XML without DTD; reqwest will later also be needed for external images and AI, from stage 14 for the token endpoints of the sign-in with Microsoft and Google (0042; PKCE with sha2, randomness from aws-lc-rs, no OAuth library) |
| Types over IPC | tauri-specta, specta, specta-typescript, pinned exactly | =2.0.0-rc.29 / =2.0.0-rc.28 / =0.0.15 | generates types, commands and events; `u64` is not silently mistyped; export via a test that fails when the file is outdated (0004). TypeScript types are never maintained by hand |
| Sync encryption | age (X25519, scrypt, feature `armor`), hmac, sha2 | 0.11.5 / 0.13 / 0.11 | reviewed format, no home-made cryptography; 0.11 instead of 0.12 because of a security advisory in the P-256 branch of 0.12 (0014). In the IMAP folder the ciphertext is stored as age ASCII armor (0022) |
| Time zone | jiff (`std`, `tz-system`, `tzdb-zoneinfo`, `tzdb-bundle-platform`) | 1.0.2 | system zone including daylight saving time for the search's date filters, on Windows with a bundled tzdb; MIT OR Unlicense (0017). `mail-content` uses only the civil date types (calendar invitations: date, weekday, date arithmetic); time zones there are resolved by the engine through the trait `ZoneResolver`, the tzdb is read in `mail-content` only in tests |
| Logging | tracing, tracing-subscriber, tracing-appender | 0.1.44 / 0.3.23 / 0.2.5 | structured logs, rotating daily (14 files) in the app's log directory; `Secret<T>` redacts |
| General (Rust) | serde, serde_json, thiserror, tokio | 1.0 / 1.0 / 2.0 / 1.54 | serialisation, typed errors, runtime |
| Tests | cargo test, Vitest, WebdriverIO with `@wdio/tauri-service` and `tauri-plugin-wdio-webdriver` | – / 5.1 / 9.31, 1.5.1, =1.5.1 | embedded WebDriver, only with Cargo feature `e2e`; runs on macOS (spike D). Not `tauri-plugin-wdio` (without `switchToWindow` it waits 5 s per command). WebDriver clicks are synthetic; real mouse clicks via a dedicated test command. Desktop tests need a GUI session and do not run in `scripts/check.sh` but as `pnpm --dir app test:e2e` |
| Accessibility (tests) | axe-core (development only) | =4.14.0 | stage 8: checks roles, names, ARIA and structure of the rendered views in Vitest (`app/tests/a11y.test.tsx`), light and dark; no serious or critical findings allowed. Colour contrasts are checked by `app/tests/contrast.test.ts` on the tokens, because jsdom does not render stylesheets; the mail in the sandbox frame is left out. MPL-2.0 (compatible with GPL-3.0, not shipped); 4.14.0 of 2026-10-05 instead of the one-week-old 4.14.1 |
| Test server | Stalwart (one binary, IMAP+SMTP+JMAP), set up via the management API by `scripts/testmail.sh` | 1.1.1 | without Docker and sudo; IDLE, CONDSTORE, QRESYNC, MOVE, UIDPLUS, SPECIAL-USE, custom keywords verified (0001). JMAP over HTTPS on 18443 with `STALWART_PUBLIC_URL` (0041). Development tool only, not shipped |
| Windows build | cargo-xwin, LLVM, NSIS | 0.24.1 / 23.1 / 3.13 | installer from the Mac, built in spike C (cold 74 s); `makensis` only with a UTF-8 locale (`LANG=en_US.UTF-8`), see `docs/SETUP.md` |

Rejected: SQLx (asynchronous without benefit with one writer, SQLCipher only via detours), SQLCipher
(see decisions), Electron (memory footprint), embedded llama.cpp (build effort per platform),
own sync server with PostgreSQL.

## Repository

```
vice-mail/
├── app/                    Tauri app
│   ├── src/                React: app/ features/ components/ services/ stores/ i18n/ styles/ config/
│   ├── src-tauri/          shell, commands, capabilities
│   └── tests/              Vitest and WebdriverIO
├── crates/
│   ├── mail-model/
│   ├── mail-store/
│   ├── mail-imap/
│   └── mail-engine/
├── testmail/               test server configuration and seed data
├── spikes/                 throwaway code from stage 0, never imported by product code
├── scripts/
├── docs/
├── Cargo.toml              workspace
└── CLAUDE.md
```

## Module boundaries

- `mail-model`: IDs, account, mailbox, message, flags, capabilities, error types and the trait
  `MailBackend`. No IO, no dependency on other crates of our own.
- `mail-store`: the only place with SQL. One write thread, a pool of read connections, WAL.
  No database call blocks a Tokio worker.
- `mail-imap`: speaks IMAP and SMTP, returns model types. Knows neither database nor UI.
- `mail-jmap` (stage 14, 0041): speaks JMAP over HTTPS behind the same traits, returns
  model types. Knows neither database nor UI; it writes its UID numbering through the
  trait `UidMapStore`, which the store implements. The engine chooses the adapter per account
  (`AnyConnector`, `AnySender` in `mail-engine/src/protocols.rs`).
- `mail-content`: turns raw data into a displayed mail: parsing, HTML sanitising with mandatory CSP,
  link placeholders, link warnings, safe file names, preview text. Pure, without IO, without Tokio;
  security-critical and tested with its own corpus (hence a fifth crate).
- `mail-engine`: connects adapters and storage. Offers the shell a facade `AppCore` and a
  stream of typed events. All behaviour is testable via `AppCore` without a WebView.
- `app/src-tauri`: passes commands through to `AppCore` and events on to the UI.
- UI: queries data page by page and reloads when an event affects the view. It never polls.
  The bindings generated by tauri-specta (`app/src/generated/`) and `@tauri-apps/api` are imported only in
  `app/src/services/ipc.ts` (ESLint `no-restricted-imports`); `invoke` appears only in the
  generated file.

**Rules for the bridge (0004)**
- Every `u64`/`i64` field defines how it arrives in TypeScript: as a number if it stays safely below
  2^53 (sizes, UIDs, timestamps in milliseconds), otherwise as a decimal string (MODSEQ).
- Framework errors (unknown command, wrong argument, ACL) arrive as a string, not as
  `IpcError`. `ipc.ts` detects this and maps it to `code: "internal"`.
- The UI receives events via the capability `core:event:default`; further capabilities only with a justification.

`MailBackend` reports via a `Capabilities` struct what the server can do (IDLE, CONDSTORE, MOVE,
UIDPLUS, SPECIAL-USE, custom keywords, from stage 14 Gmail with `X-GM-EXT-1` and JMAP with what
the adapter emulates there, 0041). The engine and the
UI ask for it. What a
server cannot do is not faked but hidden or given a clear notice.

## Data model

| Table | Content | Class |
|---|---|---|
| `account` | UUID, type, address, name, colour, server settings, reference to the keychain, `writes_enabled`; from stage 14 (0042), for accounts that sign in with the provider, the reference is `oauth:<microsoft\|google>:<id>` and the entry holds the refresh token; for JMAP accounts with an API token (0041, addendum 1) `token:<id>`, the entry holds the token | sync (without secrets) |
| `identity` | sender address, name, Reply-To, default; from stage 10 default signature for new mails and for replies (UUIDs, 0023) | sync |
| `mailbox` | server name, delimiter, parent, role (Inbox, Sent, Drafts, Archive, Trash, Junk, All, Custom), UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ | Cache |
| `message` | internal ID, account, Message-ID header, thread, subject, sender, recipients, dates, size, preview, category (according to the user's rules; plus the automatic result with its reason, important marker, normalised sender address, domain and List-Id, stage 9), reference to the body; from stage 17 (0050) Reply-To where the header block was fetched (`reply_to_addrs`) and `contacts_seen` 2 (read by the contact rules of stage 17) | Cache |
| `message_location` | message, mailbox, UID, flags, keywords, MODSEQ | Cache |
| `message.gm_msgid`, `message.gm_labels` | stage 14 (0034): Gmail's `X-GM-MSGID` (one mail across all labels) and `X-GM-LABELS` (JSON, decoded, supplemented with the label of the folder read); NULL for other servers | Cache |
| `account_server` | stage 14 (0034): what the last login said about the server, `gmail` = `X-GM-EXT-1`; not on `account`, whose trigger feeds the sync | Device |
| `thread` | account, normalised subject, last message, count, unread | Cache |
| `attachment` | from stage 20 (0060): the attachments of every message whose body (or, for a large message, its structure and text) is stored: message, index as in the reading pane, file name (also in lower case for the search), MIME type, kind (image, PDF, document, spreadsheet, archive, other), size, IMAP part of a partial body; written with the body's search index; feeds the "Attachments" view | Cache |
| `message_body`, `message_body_part` | reference to the compressed body file (whole mail or individual parts of large mails), size, checksum | Cache |
| `remote_content_sender` | senders whose external images are always loaded (account UUID, normalised address) | sync |
| `message_fts` | FTS5 index over subject, sender, recipients, text, attachment names | Cache |
| `message_fts_tri` | from stage 22 (0074, migration 0029): the same columns and rowids tokenized into trigrams, for search inside words; written with `message_fts` | Cache |
| `operation` | outbox: kind, payload, idempotency key, state, attempts, last error; from stage 21 (0063, migration 0027) also `mark_mailbox`: one row per folder marked as read, `mailbox_id` set and `uid` NULL, the UIDs as a sequence set in the payload | Device |
| `draft` | UUID, account, identity, headers, Message-ID (fixed from creation), text, quote, signature yes/no and chosen signature (stage 10), attachments and inline images (with Content-ID) as references to our own copies under `compose/`, state (`editing`/`sending`), revision, server copy (folder, UIDVALIDITY, UID, revision) (0018, 0023); `kept_original`: Message-ID of another program's draft that stays untouched in the folder because the editor could not take it over completely (0047) | Device (reconciled via IMAP) |
| `contact` | one row per address: key, spelling, counter, last used, rank (REAL with index), names, `user_name`, folded search words, `contact_group` for merged contacts (0016, 0018, 0023); from stage 17 (0050) `seen`/`last_seen` (copied, Reply-To), `personal`, `kind` (person, list, system) and the user's `note` | Cache; name, merging and note sync |
| `contact_picture` | stage 17 (0050): the picture the user chose for a contact, per normalised address: `image` (JPEG, 256 × 256 px, at most 40 KiB, written by the helper), `display` (the JPEG this device's helper wrote; for a synced picture `NULL` until then), `revision` | sync (`image` only) |
| `contact_dismissed` | rejected duplicate pairs (stage 10, 0023) | Device |
| `contact_list`, `contact_list_member` | stage 19 (0056): contact groups: UUID, name, folded name words; members by normalised address key with the address as added, the name given with it and their order (at most 500 groups, 1,000 members each). Members need no contact row | sync (kind `contactList`) |
| `signature` | pool of all signatures: name, editor HTML, text, small images (Base64, sent as `cid:`) (stage 10, 0023) | sync |
| `template`, `rule`, `saved_view` | as named; `template` with subject and editor HTML with placeholders (0023), from stage 16 with To/CC/BCC for prefilling (0033); `rule` are the local rules (conditions and actions as JSON, order); `saved_view` are the saved searches of the bar: UUID, name, query in the search language (searches in all accounts), position (stage 7, 0026) | sync |
| `recent_search` | recent searches of this device: query, last used; at most 10 (stage 7, 0026) | Device |
| `sender_rule` | correction "always as …" and important marker per address, domain or list (stage 9) | sync |
| `snooze`, `pin`, `task` | reference to the mail via account UUID and Message-ID, point in time; snooze with return time, task with done time (stage 9, 0020) | sync |
| `follow_up` | "Remind me if no reply by …" (stage 20, 0059): account UUID and Message-ID of a sent mail, due time, time of sending; `returned_at` (this device only) once it came back without a reply; nothing on the server. `draft.follow_up_at` holds the choice made in the composer until the server accepted the mail | sync (`returned_at` Device) |
| `reading_list` | reading list "Reading list": account UUID, Message-ID, added; no keyword on the server (stage 16, 0033) | sync |
| `mailbox_pref` | favourite folders (position) and folder colour (index 0–7 of the account colours) per account UUID and decoded path; only while one of them is set (stage 16, 0033) | sync |
| `blocked_sender`, `blocked_hidden` | blocked senders (normalised address); hidden mails of blocked senders on accounts without write access or without a spam folder (account UUID, Message-ID, sender) (stage 16, 0033) | sync and Device respectively |
| `attachment_pin` | pinned attachments: account UUID, Message-ID, index of the attachment, plus name, type and size at pinning time (the "Pinned attachments" view needs no body); no keyword on the server (stage 16, 0036) | sync |
| `receipt_answer`, `receipt_received` | read receipts (RFC 8098): the user's answer to an incoming request (`queued`, `sent`, `declined`) per account UUID and Message-ID; returned receipts per reported Message-ID and recipient, detected when the body is stored (stage 16, 0033) | Device and Cache respectively |
| `setting` | key, value, scope, version | sync, where registered as syncable |
| `setting` `ai.settings` | AI in the app (stage 13, 0024): switch, provider profiles (kind, base URL, model), profile per function, cloud approval per account, consent per profile and host, notices of open replies; keys only in the keychain (`ai-profile:<id>`) | Device |
| `setting` `ui.*` | UI (stage 8, 0025): changed shortcuts per platform (`ui.shortcuts.macos`, `ui.shortcuts.windows`) and text size (`ui.text_size`) sync; recently used commands of the palette (`ui.recent_commands`), state of the AI panel (`ui.ai_panel`, 0028) and layout per window kind (`ui.layout.main`, `ui.layout.message`, `ui.layout.composer`: reading pane, density, accounts-and-folders column visible (default yes) and its width 200–320 px, default 240) Device; plus `compose.default_quote_position` (position of the reply in new replies, sync like the send delay); from stage 16 (0033) `compose.archive_on_reply` and `compose.read_receipts` sync, `ui.unread_first` (views with "Unread first") Device, (0036) `ui.accent_theme` (colour theme, one of six, default blue per 0053, orange per 0038 before; a stored choice stays) sync, (0039) `ui.conversations` (conversation view, default on) sync, stage 17 `ui.column_views` (views of the column and the rail: order, shown or hidden; `[]` is the default) sync, stage 19 (0061) `ui.list_orders` (order per view; newest first is not stored) and `ui.list_table` (rows or table, columns shown and their widths) Device | sync and Device respectively |
| `setting` `desktop.background`, `desktop.close_hint`, `desktop.texts`, `notifications.paused_until` | stage 21 (0069): keep running when the main window closes (default on) and the macOS menu-bar icon (default on); whether the one-time note on closing was shown; the tray menu's texts in the UI language of the last start (for a start in the background); the end of a notification pause (Unix ms) or `null`. "Start with the system" is not stored here: the system's login item is the truth (0070) | Device |
| `setting` `reading.block_trackers`, `reading.clean_links`, `compose.attachment_reminder` | stage 20 (0058): block tracking pixels and images of known tracking services (default on), strip tracking parameters from links when they are described and opened (default on), ask before sending when the text speaks of an attachment and none is there (default on) | sync |
| `ai_request` | request log of the in-app AI, metadata only (function, profile, host, local, model, mails, bytes, result, error code, duration; 30 days, at most 10,000) (stage 13, 0024) | Device |
| `open_reply_dismissal` | dismissed notices of open replies (stage 13, 0024) | Device |
| `mcp_state`, `mcp_account`, `mcp_client`, `mcp_access` | AI via MCP (stage 12, 0021): main switch and Windows pipe name, approval per account, registered clients (key in the keychain, only the reference here), access log with metadata only (30 days, at most 10,000 entries) | Device |
| `sync_record` | per syncable record the current version (content or tombstone, for accounts with field versions) and the last reconciled one; `sync_dirty` holds those changed in a transaction until the commit (stage 11, 0022) | Device |
| `sync_state`, `sync_adopted` | device ID in the sync, highest version of the clock, on/off, sync account and folder, last pass and last full check, folder state; adopted accounts without password | Device |
| `sync_device` | devices of the sync with name, system, last seen | sync |
| `account.protocol` | stage 14 (0041): `imap` or `jmap`, fixed from setup onwards; for JMAP the `imap_*` columns name the JMAP server, `smtp_*` a copy | sync (in the record only for JMAP) |
| `jmap_state`, `jmap_mailbox`, `jmap_uid` | stage 14 (0041): the JMAP adapter's UID numbering per account (JMAP account id, Email state, counter as HIGHESTMODSEQ), per JMAP mailbox (UIDVALIDITY, next UID, numbering complete) and per UID (Email id, counter value of the last change) | Cache |
| `account_server_change` | servers (IMAP and SMTP) that an account record from the sync folder names for an existing account and that differ from this device's; they wait for "Use the new servers" or "Keep the current servers" (`dismissed`), and until then the sync regards these servers as the account's (security review F1, 0040) | Device |

Classes: **Cache** can be rebuilt from the mail server. **Device** stays on the device.
**sync** travels via the settings sync. A table is only reconciled if it is
explicitly registered as syncable.

The same mail in two folders is one row in `message` and two in `message_location`. On
Gmail, `X-GM-MSGID` merges the locations instead of the Message-ID (stage 14, 0034).
Flags belong to the location, as in IMAP. Records that travel between devices never refer to mails
by local IDs or UIDs, but by account UUID and Message-ID header.

Migrations are forward-only. Before each migration the database file is copied; the last three
copies are kept. A database newer than the app knows is not opened but rejected with
a clear message. Whatever prevented opening at startup is fetched by the UI's app shell via
`getStartupProblem` (error, database file, backup copies, newest first) and shown above
every view with the way back to a copy and to "Export diagnostics…" (2026-11-24).

**Schema and queries (measured in spike B, `docs/decisions/0003-sqlite-schema-und-suche.md`)**
- `message.id` is the sort date in seconds; if that number is taken, the next free one. It is
  assigned in the write thread. It is also the rowid of the FTS index: search hits come newest
  first without sorting, a date filter is a rowid range.
- Sort date: the `Date` header if it is at most 24 hours after INTERNALDATE, otherwise INTERNALDATE.
- `message_location` is `WITHOUT ROWID` with key `(mailbox_id, message_id)`, flags as separate
  columns. Indexes: unique on UID per mailbox, on `message_id`, partial index on unread
  (`WHERE seen = 0`); plus `message` by Message-ID and thread, `attachment` by message.
- `message_fts` is contentless with `contentless_delete=1`, columns `subject, from, to, cc, body,
  attachments`, tokenizer `unicode61 remove_diacritics 2`. Bodies are indexed afterwards via
  `INSERT OR REPLACE`. Substring search (the German "auszug" finding "Kontoauszug", bank statement)
  came with stage 22 (`docs/decisions/0074-kontakte-in-der-suche-und-suche-im-wort.md`): a second
  contentless index `message_fts_tri` (tokenizer `trigram remove_diacritics 1`, the same folding)
  over the same columns, the mail text included, written in the same transactions, rebuilt and
  swapped together with `message_fts`; the job target `message_fts_tri` fills it for existing
  mail after migration 0029. An unquoted term of three or more characters matches in either index,
  shorter and quoted terms only as words; a single word uses the trigram index alone once it is
  complete (every word hit is a substring hit there). `CompiledQuery::text` is a `TextMatch` of
  clauses per index; the store walks one clause's `MATCH` by `rowid DESC` (two clauses that need
  both indexes as a `UNION` merged in rowid order) and checks the others per row with
  `IN (… MATCH ?)`. Cost: about 3.7 bytes of index per byte of text (100,000 mails with 17,935
  local bodies: database 119 → 272 MB), first hits at most 29 ms in a debug build.
- Lists only via keyset, never via `OFFSET`. Views across several mailboxes (unified inbox)
  via `UNION ALL` per mailbox, never via `IN (…)` (measured 0.14 ms versus 393 ms). Fixed
  join order via `CROSS JOIN`. For every list and search query a test checks the query plan.
- First search hits by `rowid DESC`; `bm25` at most as an option.
- From stage 2: after `ANALYZE` with only one account, the query planner chooses a scan for a join on
  `account`, despite `CROSS JOIN`; the account of a mailbox is therefore fetched via a scalar
  subquery. Read connections load statistics only when they are opened, so plan tests check
  reader and writer. A window is stored in ascending order of (sort date, UID). Additional
  indexes: `mailbox_role` (unique per account, if set), `mailbox_path` (order of the
  sidebar). Measured via the store API: 100,000 headers in 1.1 s, list page 0.07 ms.
- FTS rows are inserted per transaction in ascending rowid order, even when the sync runs from new
  to old (descending costs a third of the insert rate).
- Write connection: WAL, `synchronous=NORMAL`, `foreign_keys=ON`, 64 MB cache, `mmap_size` 256 MB,
  `journal_size_limit` 64 MB, `temp_store=MEMORY`. Read connections (2 to 4): `query_only`,
  8 MB cache, `mmap`. Sync in windows of 500 mails with `BEGIN IMMEDIATE`.
- From stage 7 (`docs/decisions/0017-suche-threads-integration.md`): index and thread are written in the
  transaction that stores the message or body (body text via a `BodyTextSource` of the
  engine, at most 256 KiB per mail); existing mails and a rebuild run as a job in the
  background, the rebuild with a swap at the end. `thread` holds count and newest message, the
  unread ones are counted by the thread list when reading; the thread list of a mailbox shows per thread the
  newest mail in that mailbox. Highlights are computed by Rust (contentless index). Account groups
  (sync, via account UUID) and hidden accounts (Device) in separate tables. Measured in
  `docs/testing/performance.md`: first search hits with 500,000 mails 0.3–17 ms, thread list 0.3 ms;
  a sync window costs 4.7 ms more than in stage 2.
- From stage 9 (`docs/decisions/0020-posteingang-komfort.md`): new mails are categorised in the
  transaction that stores them (rules from `mail-content` via a `Classifier` of the
  engine, local signals from the database); changed sender rules and existing mails run as a
  background job, newest first. Category and important marker are also stored on the location
  (`loc_category`, partial index `loc_important`; a new location takes them over on insert, changes
  via trigger), so that lists by category run via keyset.
  Snooze, pins and tasks refer to mails via account UUID and Message-ID; the inbox list leaves
  them out per row via the unique index. Measured: categorising costs 2.5 ms per window of 500
  headers; list pages stay under 0.3 ms.
- From stage 16 (`docs/decisions/0033-funktionsabgleich-kern.md`): "Unread only" is a
  range of `loc_unseen`, "Unread first" is this range followed by the read locations via
  the primary key (`seen = 1` as a filter, no index of its own). The inbox lists leave out
  hidden mails of blocked senders per row via the primary key of
  `blocked_hidden`; reading list and folder settings are one index lookup each.
- From `docs/decisions/0039-konversationen-im-sortierten-posteingang.md`: the conversation variant
  of every inbox list is the message list of the same kind, grouped by thread, one row
  per thread at its newest member that would appear in the same list (`NOT EXISTS` over a
  range of `message_thread` and the key of the location); keyset via the mail ID of the row.
  Threads with a pinned or returned mail appear only at the top (check per thread member
  via the unique indexes of `pin` and `snooze`). Per row, count and unread are counted over
  all mails of the thread (`message_thread`, `loc_message` via `INDEXED BY`), participants, attachment and
  flag come from the newest 200 mails. No migration.
- Contacts (stage 17, `docs/decisions/0050-kontakte-bilder-spam.md`): every address of From, To,
  Cc and Reply-To becomes a contact; only own mail and mail from people rank. Messages read by the
  stage 10 rules (`contacts_seen = 1`) are read again by the background job through the partial
  index `message_contacts_backfill` (`INDEXED BY`), adding only what stage 10 left out. A
  contact's mail and the "mark as spam" move use `message_sender` (sender key, id). The view's
  search is the directory's word-prefix search over names, addresses and domain labels: under
  10 ms at 10,000 contacts (debug build).
- Sorted and filtered views (stage 19, `docs/decisions/0061-sortieren-und-tabellenansicht.md`,
  `0062-schnellfilter.md`): `message_location` carries the sort keys of its mail (`sort_sender`,
  `sort_recipient`, `sort_subject`, folded in Rust: lower case, Latin accents removed, leading
  punctuation dropped, at most 40 characters; and `size`), each with an index
  `(mailbox_id, key, message_id)` (migration 0026). A page in another order is a range of that
  index (`INDEXED BY`, keyset over the row value `(key, message_id)`, ties by date in the same
  direction); several mailboxes merge by `UNION ALL` as before. The quick filter is the search's
  compiled filter (`search::conditions_on`, now with a message alias) plus tag and contacts,
  checked per row; the text as `IN (… MATCH ?)`. Conversations: a location is its thread's row
  when no newer mail of the thread in the mailbox matches the same filter. Inboxes and unified
  views leave out waiting snoozes and blocked senders. Newest first without a filter keeps the
  lists above (sections, bundles, priority, "Unread first"). Every list row reads `m.size` as
  column 14 (`MessageListItem.size`). Commands: `listViewMessages`, `listViewConversations`
  (cursor `ViewCursor`), `countListView`, `listViewTags`, `get/setListOrder(s)` (device setting
  `ui.list_orders`), `get/setListTable` (device setting `ui.list_table`).
- "Flagged" view (SPEC, 2026-11-24): `\Flagged` (JMAP `$flagged`) per mailbox of the accounts except
  trash and spam as a range of the partial index `loc_flagged`, merged via `UNION ALL`; every mail
  once, in the inbox if it is flagged there, otherwise in the newest folder in which it is flagged;
  at most 1,000 in one answer like the other mark views (`listFlagged`). No migration.
- Stage 20 (`docs/decisions/0059-erinnern-wenn-keine-antwort-kommt.md`,
  `docs/decisions/0060-anhaenge-ansicht-und-postfach-speicher.md`): a reply to a mail with a
  reminder is a message of its thread (via `thread_ref`, so also when the own copy is not stored)
  in any folder of the account, sorted after the sending and not from an own address; checked one
  second after the last `messagesChanged` of an account with reminders, when a reminder falls due
  and when one is set. The "Attachments" view walks `attachment` backwards by primary key or by
  `attachment_kind (kind, message_id)` with keyset `(message_id, idx)`; account, file name and sender
  are filters on that walk, Trash and Junk are left out. Measured (debug build, 20,000 attachments):
  a first page with any filter under 400 ms, a test checks the plan (no temporary B-tree).
  The search language knows `larger:` (`größer:`; sizes with K, M, G, B; `-larger:` is smaller),
  local only, over `message.size`.
- Contacts in the search (stage 22, `docs/decisions/0074-kontakte-in-der-suche-und-suche-im-wort.md`):
  the search field is a combobox that offers up to three contacts while typing (the directory's
  word-prefix search, `listContacts` with every kind). Choosing one writes `contact:<address>`
  (`kontakt:`), a filter of the search language that the engine resolves on every search to all
  addresses of the contact (merged ones too), so saved searches follow merges; it compiles to
  phrases in the word index's columns `from`, `to` and `cc` (word boundaries: `an@firma.de` does not
  find `jan@firma.de`). On the server it becomes `OR FROM (OR TO CC)` with the written address.
- From the re-measurement of stage 8/9 (`docs/decisions/0029-start-ohne-breites-lesen.md`): queries on
  a partial index that becomes empty after a catch-up run (`message_category_todo`,
  `message_contacts_todo`) name it via `INDEXED BY`. Otherwise `PRAGMA optimize` on close
  records the size from before the catch-up, and the planner reads the whole table to find no
  row. Background jobs that find work at startup only begin after the
  startup, so that they do not occupy the write thread before the startup's writes.

## Mail engine rules

**Sync per mailbox**
- First sync from new to old in windows, so that the inbox is usable immediately.
- After that: new UIDs above the last known one; flag changes via CONDSTORE, otherwise via
  refetching window by window; deletions via comparing the UID sets.
- If UIDVALIDITY changes, the mailbox's mapping is discarded and rebuilt. Local
  data about mails is kept via the Message-ID.
- IDLE on the inbox of each account, other folders at an interval. After sleep or a
  network change reconnect, with exponential backoff and the option to cancel.
- All data is never refetched when a targeted sync is enough.

**Sync tasks (stage 2, 0008)**
- Per account a supervisor task with the state machine (`sync_state.rs`). Connected: worker
  (main connection, passes), IDLE watcher (own connection, sections of 5 minutes),
  poller (full pass every 3 minutes, without IDLE the inbox every minute). A
  failed connection ends the session; the state machine decides on backoff or stop.
- One pass per mailbox: EXAMINE, on a new UIDVALIDITY `reset_mailbox`; unchanged (UIDNEXT,
  HIGHESTMODSEQ, count equal) is skipped; otherwise UID comparison for deletions, flags via
  `CHANGEDSINCE` or window by window, new UIDs up to the UIDNEXT of the EXAMINE, then the rest of the
  initial sync. Orphaned messages are deleted only after a full pass without initial sync and
  without a skipped mailbox.
- A panic in a session becomes the account error `protocol` with backoff, never the end of the app.
- Sleep: a jump between wall clock and monotonic clock (over 20 s) makes all accounts
  reconnect; in addition `AppCore::resume()`/`network_changed()` for the shell. There is no OS signal
  (it would need `unsafe`).
- Events: `accountStatusChanged` always carries the complete status; per stored
  window one `messagesChanged` and one `mailboxesChanged`; `newMail` only outside the
  initial sync, with the number of new unread mails.

**Outbox**
- Every writing action first changes the local state, is recorded as an `operation` and
  then executed. States: `queued`, `in_flight`, `confirmed`, `failed`, `conflict`.
- Operations are phrased idempotently: "set flag" instead of "toggle flag"; a move
  checks before the retry whether the mail is already at the target.
- After a crash, `in_flight` operations are first checked against the server, then retried.
- From stage 16 (0033), **Send and archive:** `SendJob.archive_original` (per draft, for
  replies via the setting `compose.archive_on_reply`) schedules, only in the transaction that confirms the
  acceptance, the action `archive` for the locations of the original in the batch of the send; without
  an archive folder the original stays, the send still counts. **Read receipts** go out only on
  the explicit command `send_read_receipt`, as `send` without a draft and without a copy in
  "Sent"; after that `$MDNSent` on the original. Incoming requests are detected on reading,
  never on arrival (wire-tap test against Stalwart in `crates/mail-engine/tests/compose.rs`).
- From stage 16 (0033), **between accounts:** copying and moving to another account is an
  `append` in the outbox of the target account (`AppendSource::Transfer`, frozen bytes, flags of the
  source without keywords). Both accounts need write access (for copying only the target),
  the mail needs a Message-ID. Only when the copy is confirmed in the target and found via the Message-ID
  above the mark does the same transaction record an `expunge` in the source account;
  after a crash the `append` is checked, not repeated (never two copies, never
  lost). Server test with fault injection in `crates/mail-engine/src/outbox/transfer_server_tests.rs`.
- Stage 18 (task D), **creating folders:** `createFolder(account, parent | null, name)` checks the
  name in the store (not empty, at most 200 characters, no control character, not the server's
  delimiter, not taken at that place, a parent of the account that has a delimiter and no
  `\Noinferiors`, write access) and computes the decoded path: below the parent, or at the top
  of the account, below the inbox when every other folder lies there (`INBOX.` namespaces). It is
  recorded as `create_mailbox` and created like the folder for a missing role (IMAP `CREATE`, JMAP
  `Mailbox/set`); the folder list is read right after. `reloadFolders(account)` asks the
  account's sync for a full pass now (folder list included; not connected: a reconnect), so
  folders made by other programs show without waiting for the 3-minute poll.
- From stage 19 (0055), **redirecting:** the original's bytes with a `Resent-Date/-From/-To/-Cc/
  -Message-ID` block in front (`Bcc`, `Delivered-To` and `Return-Path` of the original dropped,
  no `Resent-Bcc`), from one of the account's identities, as a `send` without a draft
  (`SendJob.redirect`) after the undo delay; `cancel_redirect` undoes it within the delay. No copy
  in Sent; `$Forwarded` on the original once accepted. Only with write access.
- From stage 19 (0057), **importing an mbox file:** every message is an `append` of the target
  account (`AppendSource::Transfer` without a source to remove, frozen bytes, flags from Thunderbird's
  `X-Mozilla-Status` or `Status`/`X-Status`, else seen), recorded as `copy`. A message without
  Message-ID gets one in front (`<random>@mbox-import.invalid`) so the crash check can find it. The
  file is read line by line on the blocking pool, one message at a time (at most 64 MiB); at most 50
  appends wait at once. Background job with `mboxJobChanged` and cancel (the waiting appends leave the
  outbox). Only with write access, checked before anything is read.
- From stage 21 (0063), **marking folders as read:** `markMailboxesRead(account, mailboxes)`
  records one `mark_mailbox` operation per folder with unread mail, all in one batch: the
  folder's UIDVALIDITY and the UIDs that were unread then (sequence-set text in the payload). The
  same transaction marks exactly those locations read (counters drop at once); after sync writes
  the folder's pending marks are applied again in recorded order with the per-message flag
  operations. Executed as `UID STORE <set> +FLAGS (\Seen)` on the recorded UIDs only (never
  `1:*`: mail that arrived later stays unread; no `UNCHANGEDSINCE`), in parts of at most 250
  ranges and 5,000 UIDs; JMAP through the same `store_flags` (`Email/set` in `maxObjectsInSet`
  batches). In flight after a crash it is simply sent again; a refused part fails it and reverts
  the folder, another UIDVALIDITY or a missing folder is a conflict. Undo clears `\Seen` on the
  same set (refused once the folder was rebuilt); retry sends the same set. The account's "Mark
  all as read" passes its inbox and own folders, the unified inbox one call per account.
- From 2026-11-24 (alignment with SPEC), **renaming and deleting folders:** only the user's own folders (without
  a role), only with write access, one change per folder at a time; deleting only without subfolders
  and with a confirmation that names the mails stored here. `rename_mailbox` and
  `delete_mailbox` (migration 0019) change nothing locally until the server has confirmed; the
  folder column shows the pending change. After `RENAME` the rows of the folder and
  its subfolders take over the new name (mails, UIDVALIDITY, favourite and colour stay), then
  the folder list is read again. After a crash a `RENAME` counts as done if the old
  name is missing and the new one is there, a `DELETE` if the folder is missing. Before that the folder
  is left without EXPUNGE (`UNSELECT`, otherwise `EXAMINE INBOX`, never `CLOSE`). JMAP accounts are rejected by the
  core with `unsupported_server` before anything is recorded: behind the traits the JMAP adapter
  does not offer `Mailbox/set` for renaming or deleting yet.
- If something fails, the local state is rolled back and the error is shown. In a
  conflict the server state wins, and the user is told.

**Sending**
- The Message-ID is created when the draft is created and stays the same across all attempts.
- If the connection breaks after the mail was transmitted but before it was confirmed, it is not
  resent automatically. The app searches for the Message-ID in the Sent folder; if it remains unclear, it asks the user.
  Note: the mail is only there if the server itself files a copy (e.g. Gmail); with all
  others the search finds nothing, and the app asks. The SMTP adapter distinguishes "nothing
  transmitted" (safe to retry) from "unclear" (connection gone after the end marker).
- The generated message does not reveal the program (no `User-Agent`/`X-Mailer`, EHLO with
  `[127.0.0.1]`); `Date` in UTC; the same draft yields byte-identical messages.
- After successful sending a copy is filed in "Sent", unless the server does this itself
  (setting `auto`: Gmail, detected by the server name or by `X-GM-EXT-1`, 0034).
- From stage 5 (0018): sending is the outbox operation `send`. The bytes are built once on click
  and frozen, the draft is locked until the result; `not_before` holds the
  delay for undoing (cancelling deletes the operation, nothing goes to the network).
  Unclear means `failed` with `send_unclear` and a question to the user (`sendNeedsDecision`);
  it is resent only on their explicit decision. After the acceptance follow, in
  the same transaction, the Sent copy (`append`), removal of the draft copy (`remove_copy`)
  and `\Answered`/`$Forwarded` on the original. Drafts go as `append` into the Drafts folder
  (append the new version, remove the old one only if the Message-ID matches). Drafts of other programs
  keep their inline images as inline parts; what the editor cannot display leaves the
  original untouched, and the app's version sits next to it with its own Message-ID (0047).
- From stage 10 (0023), **Send later locally:** a `send` operation with the chosen time as
  `not_before` (`SendJob::scheduled`), built immediately and dated with that time; it is sent
  only if the app is running and online then. At startup, before outbox and accounts run,
  scheduled sends that were missed by more than an hour are not sent but
  turned into a question (`send_missed`: send now, reschedule, keep as draft); up to an hour
  they go out. A send is only started when it is due according to the worker's clock
  (`begin_due_operations`); the worker waits at most 30 s at a stretch (sleep).
- **SIZE:** the adapter reads the SMTP server's limit on request from a second EHLO
  after login (nothing is sent); above a known limit, sending is rejected with
  `message_too_large`.
- **Inline images** go as `multipart/related` around `multipart/alternative`, only those the
  HTML still shows; **Forward as attachment** attaches the whole message as `message/rfc822`
  (7-bit-clean ones unencoded).

**Connections (0002)**
- Every connection has read and write deadlines (default 30 s); a silent connection is detected, not waited out.
- Login via LOGIN; if user name or password contain non-ASCII characters, via
  `AUTHENTICATE PLAIN` (UTF-8, with SASL-IR if offered), see 0008. Accounts that sign in with
  Microsoft or Google (0042) log in with the access token via `AUTHENTICATE XOAUTH2`, via
  `OAUTHBEARER` (RFC 7628) where a server offers only that; SMTP likewise (`AUTH XOAUTH2`). A token
  never goes over LOGIN or PLAIN.
- After login `ENABLE CONDSTORE`, on QRESYNC-capable servers `ENABLE QRESYNC`.
- An aborted command closes the connection; it is never reused with half-read responses.
- Unsolicited responses are collected after every command (the library buffers only 100).
- Certificate errors are typed: not trusted, wrong name, expired.

**Read-only mode**
- Accounts with `writes_enabled = false` open mailboxes with EXAMINE. The adapter rejects every
  writing command with a typed error. A test proves this.
- Even with write access enabled, sync, IDLE, reading pane and
  connection test always connect read-only (EXAMINE, PEEK); writing happens exclusively over the
  outbox's own connection (wire-tap test in `crates/mail-engine/tests/actions.rs`).
- Gmail (stage 14, 0034): `X-GM-MSGID` and `X-GM-LABELS` are FETCH data items and pass with
  EXAMINE through the allowlist; labels are changed only by the outbox connection (`UID STORE ±X-GM-LABELS`,
  `Access::ReadWrite`), the read-only and the sync connection reject it (tests in
  `crates/mail-imap/src/tests/gmail.rs`).
- JMAP (stage 14, 0041): a read-only connection sends only methods of an allowlist
  (`Core/echo`, `*/get`, `*/query`, `*/changes`, `*/queryChanges`, `SearchSnippet/get`,
  `Identity/get`, from stage 20 `Quota/get`); `Email/set`, `Email/import`, `Mailbox/set`, `EmailSubmission/set` and uploads
  fail with `read_only` before anything is sent. Writer and sender reject a read-only
  configuration before any connection (tests in `crates/mail-jmap/src/tests.rs`, wire-tap of the
  methods in `crates/mail-engine/tests/jmap.rs`). The connection of the settings sync (0041,
  addendum 2) is always read-only; its own writes are checked by a scope on their arguments
  before sending: `Mailbox/set` only to create the sync folder, `Email/import` only into it,
  `Email/set` only destroying listed mails of the folder or taking them out of it
  (`crates/mail-jmap/src/sync_folder.rs`).
- Storage quota (stage 20, 0060): `GETQUOTAROOT` and `GETQUOTA` (RFC 9208) are on the read-only
  allowlist; the reading connection asks `GETQUOTAROOT INBOX` only when the server announces
  `QUOTA`, JMAP asks `Quota/get` (RFC 9425) only with `urn:ietf:params:jmap:quota`. The answer is
  kept for 15 minutes. `NO [OVERQUOTA]` and the JMAP SetError `overQuota` become the error code
  `over_quota` (not transient).
- Exception, only after "Turn on sync" (stage 11, 0014, 0022): the connection of the
  settings sync may write to exactly one folder, the sync folder; every other
  writing line is rejected by the adapter with `read_only` before it is sent
  (wire-tap test in `crates/mail-imap/tests/sync_folder.rs`).
- After every step of the outbox a hook immediately triggers a sync of exactly the affected folders;
  the UI gets one bundled event `operationsChanged` per change.

**Offline**
- Headers of all mails are stored locally. Bodies are fetched on opening and preloaded for a configurable
  period (default 30 days for Inbox, Sent, Drafts and Flagged).
- Bodies are stored zstd-compressed as files under `bodies/<account UUID>/` in the app's data directory,
  written atomically, the database holds the reference (measured in 0009: reading as fast as
  BLOB, database 2.3 MB instead of 57 MB with 460 bodies). Small preloaded mails of an account are
  written in packs of up to 1 MiB. Mails over 2 MiB are fetched partially: header and text parts
  first, attachments on request. If a file is missing or damaged, it is fetched again.

**Threads**
- Per account, via Message-ID, In-Reply-To and References. Fallback to the subject only cautiously:
  same normalised subject, shared participants, narrow time window.
- Conversation view (0039, default on, synced setting): inbox, unified
  inbox, folders and search results show one row per conversation. Bundles contain
  conversations. Pinned and returned conversations appear at the top, once; pending
  snoozes and hidden mails act per mail. Actions on a conversation row affect the
  mails the core names (`conversationTargets`): move, delete, snooze the mails in the
  displayed mailbox; read, unflag, unpin all mails; unread, flag, pin the
  mail of the row. Execution goes through the outbox like every action.
- Unfolding conversations (stage 17, 0049): a list of conversations is a tree in the UI.
  The messages of an unfolded row are `getThread(row message, row mailbox)`: every message
  of the thread in the account, oldest first, each at its location in the row's mailbox
  where it lies there (newest 1,000), exactly the set the row's count counts; loaded on
  unfolding, keyed by the row's thread. The reading pane shows one message (the oldest
  unread, else the newest); opening marks that message read. Actions on a child row act on
  that message alone. No new command, no migration. A conversation row shows the
  conversation's subject (its oldest message's without reply and forward prefixes,
  `display_subject`), which thread lists, inbox summaries and search hits carry; the row's
  message keeps its own subject, which search matches.

**Gmail via IMAP (stage 14, block A, 0034)**
- Detected by `X-GM-EXT-1` at every login (recorded in `account_server`), for Sent and
  hints also by the server name (`imap.gmail.com`). Providers at a glance: `docs/PROVIDERS.md`.
- On Gmail, header fetching and flag sync fetch `X-GM-MSGID`/`X-GM-LABELS` in the same FETCH line.
  One mail across all labels and "All mail" is one `message` row; on Gmail `newMail` counts
  only new rows. Records of the sync folder (a label there) stay out of "All mail".
- Changing labels is `SetFlag` with `Flag::Label` (actions `addLabel`/`removeLabel`), local
  effect on the mail, always sent, without `UNCHANGEDSINCE`.
- Archiving removes `\Inbox` from the location in the inbox (no archive folder needed), undo via
  "All mail"; moving out of "All mail" only sets the target label (no EXPUNGE there).

**JMAP (stage 14, block D, 0041)**
- One adapter behind the IMAP-shaped traits, not a second engine: the adapter numbers the
  mails of every JMAP mailbox itself (UIDVALIDITY per mailbox, UIDs in the order of
  arrival), HIGHESTMODSEQ is a counter per account that rises with every new Email state.
  "Opening a mailbox" reads `Email/changes` since the remembered state and the `mailboxIds` of the
  changed mails; a restart keeps the numbers (tables `jmap_*`).
- Push via EventSource (RFC 8620, 7.3) in `idle`; without EventSource the engine polls.
- Writing via the outbox as with IMAP: `Email/set` (keywords, `mailboxIds`, destroy),
  `Email/import`, `Mailbox/set`; sending via `EmailSubmission/set`, the Sent copy is filed by the
  app. Conflicts via the counter value per mail.
- The sync account of the settings sync must speak IMAP.

**Sign-in with Microsoft and Google (stage 14, blocks B and C, 0042)**
- OAuth 2.0 authorization code with PKCE (S256) in the system browser, redirect to a
  loopback listener of the core on a free port (`http://localhost:<port>` for Microsoft,
  `http://127.0.0.1:<port>/` otherwise). Only `GET /` with the `state` that was sent is accepted; everything
  else gets 400/404, and the core keeps waiting (5 minutes, cancellable). Module
  `mail-engine/src/oauth/`, commands `getOauthAvailability`, `addOauthAccount`, `signInAgain`,
  `cancelOauthSignIn`.
- Profiles per provider (endpoints, scopes, allowed IMAP/SMTP servers); client IDs (and Google's
  non-confidential installed-app secret) from the build environment (`MAIL_APP_OAUTH_*`, read in
  `app/src-tauri/src/oauth.rs`). Without a client ID the provider is not available in this build.
- Tokens behind `Credentials`: refresh token in the keychain (in pieces of up to 1,000 characters because of
  Windows), access token in memory, renewed five minutes before expiry and once after a
  rejection; `invalid_grant` is `oauth_sign_in_required` and stops the account until "Sign in
  again". A token exists only for the provider's servers; `Credentials::get` never hands out such
  entries as a password.
- Gmail and Microsoft stay with IMAP and SMTP (no Gmail API, no Microsoft Graph).
- Removing an account revokes at Google (RFC 7009) and deletes the entries. Tokens never travel via
  the settings sync; the account record names only the provider (`"oauth"`), another device
  adopts the account as `oauth:<provider>:synced:<random>` without a token and signs in itself.

## Security boundaries

**HTML in the composer (stage 10, 0023)**
- Outgoing HTML, signatures and templates go through the allowlist of `mail_content::mail_html`
  (paragraphs, headings h1–h3, alignment, emphasis, lists, quotes, code, links only
  `http`/`https`/`mailto`). Images only as `cid:` with strict Content-ID syntax, and only in the
  app's own editor content; their bytes are read by the core from files the user
  chose in the system dialog or dragged onto the window. Pasted HTML (Word, web pages) is cleaned by the core
  (`cleanPastedHtml`): no script, no style, no image, no external reference.
- Stage 21 (0067): outgoing HTML carries no text or background colour (the editor offers none,
  pasted and quoted colours never reach the model; code blocks have a grey line, not a light
  background) and declares `<meta name="color-scheme" content="light dark">` and
  `supported-color-schemes`. Quotes are built from the stored original, never from the reading
  pane's rendering.

**HTML from mails**
0. A linear pre-check rejects HTML that would make html5ever quadratically slow or inflate the
   output (more than 150,000 tags, depth over 1500, too many open formatting tags);
   such mails are shown as text. It must read the HTML the way html5ever's tokenizer does
   (comments, attributes, raw text only for HTML elements, not in SVG and MathML, CDATA there);
   stage 15 found three differences (`docs/SECURITY_MODEL.md`, C-1), and since then a differential test in
   `crates/mail-content/tests/fuzz.rs` compares it with the parser.
1. Sanitise in Rust with ammonia: no script, no event attributes, no forms, no
   `meta refresh`, no `base`, no embedded objects or frames.
2. Replace external addresses in images, `srcset`, backgrounds and CSS with placeholders as long as
   the user has not approved them. Inline images (`cid:`) come from local storage.
3. Display in a `srcdoc` `iframe` with `sandbox="allow-same-origin"` and nothing else, never
   `allow-scripts` (with both together, a script in spike D reached `parent.__TAURI_INTERNALS__`).
   Every `srcdoc` begins with the mandatory meta CSP `default-src 'none'; img-src data:; style-src
   'unsafe-inline'; base-uri 'none'; form-action 'none'` and `<meta name="referrer"
   content="no-referrer">`, followed by a base style (`MAIL_DEFAULT_STYLE`, only the `font-family` of the
   system font as `:where(html)`, without a URL) that the mail's own style overrides. Images only as `data:` that the core wrote itself: inline images, approved external images and `data:` URLs in the HTML (recognised by their bytes as PNG, JPEG, GIF or WebP; never SVG, BMP, AVIF, ICO) are re-encoded in the helper (security review C-4, 0046, "Images in mails" below); the WebView's image decoders never see bytes from a mail. The frame additionally inherits the
   app's CSP; WebKit does not know the `csp` attribute. No JavaScript runs there, and Tauri injects its scripts only into the
   main frame, so the Tauri interface is not reachable (0005). The height of the frame
   is measured by the parent window: `contentDocument` is readable (same-origin), a `ResizeObserver` of the
   parent window on the root element in the frame fires under WKWebView (checked in stage 3); re-measuring
   happens via `setTimeout`, not via `requestAnimationFrame` (does not run in hidden windows).
4. Approved external images are loaded by the Rust core: without cookies, without referrer, with a neutral
   identifier, with size and type limits.
   Trackers (stage 20, 0058, setting `reading.block_trackers`, default on): the sanitiser counts
   tracking pixels (external images of at most 2 × 2 px, a zero side, or hidden), images and CSS
   images of known tracking services (`mail_content::trackers::KNOWN_TRACKER_HOSTS`) and images
   with tracking parameters. Pixels and known services are never loaded, not even after "Load
   once" or a sender approval: they are not listed as remote resources and keep the placeholder.
   An image with tracking parameters loads, when approved, from its address without them. The
   reading pane shows the count with an explanation.
   Colours (stage 21, 0066): the colour pass (`mail_content::colors`) runs after steps 1–2, on the
   sanitised stylesheet and on every sanitised attribute value, with the same CSS tokenizer. It may
   change exactly three things: replace colour values (named, `#hex`, `rgb[a]()`, `hsl[a]()`) of
   colour properties and of `bgcolor`/`color`/`text`/`link`/`vlink`/`alink` with colours it
   writes itself (`#rrggbb` or `rgba(r, g, b, a)`), remove `color-scheme` and
   `supported-color-schemes` declarations, and decide `@media (prefers-color-scheme: …)` queries
   for the canvas (drop the condition or the whole block). Everything else is copied byte for byte;
   it adds no property, selector, at-rule, attribute or URL, never looks into `url()`, and the
   result still goes through the self-check of step 4 of the pipeline. Colours are only adapted for
   the dark scheme the UI asks for (`getMessage(…, scheme)`), unless the mail declares dark
   colours of its own or `only light`; the dark default style (`MAIL_DARK_STYLE`, after
   `MAIL_DEFAULT_STYLE`, `:where()` only) sets the frame's scheme, canvas, text and link colour.
   The hostile corpus and the fuzz test render every mail in both schemes.
5. Links never open directly. A click shows the real target and warns on a mismatch with the
   link text or on suspicious domains. Opening happens in the system browser, only `http`, `https`, `mailto`.
   The only interception point is the navigation hook in Rust (`on_navigation`): in a frame
   without `allow-scripts` WebKit calls no event listeners, not even those of the parent window. The sanitiser
   therefore rewrites every `href` to `https://link.invalid/<Render-Token>/<Index>` (only the core
   knows the real target; a placeholder with a foreign token from the mail itself is rewritten too)
   and removes `<link>`, `target`, `ping` and `download`: `<link rel=preconnect>` and clicks on
   `target=_blank` would otherwise open TCP connections that neither sandbox nor CSP prevent (0005).
   Links in AI outputs (0028) are found by the core itself in the text it returns, and it keeps them
   under a token of their own in the same register (at most 32 results alongside the 32
   renderings); the UI gets segments and opens a link only via
   `describeLink`/`openLink` with token and index, never with a URL.
   From stage 20 (0058, setting `reading.clean_links`, default on) the core removes tracking
   parameters (`utm_*`, `fbclid`, `gclid`, `mc_eid`, … in `mail_content::trackers`) from the target
   it describes and opens; the other parameters keep their order and spelling. Redirect links of
   tracking services are never resolved (that would call the service).
6. For displaying mails, the app's CSP allows `style-src 'self' 'unsafe-inline'` (otherwise no
   mail CSS applies) and `frame-src 'self' http: https:` (otherwise WebKit discards link navigations before the
   hook sees them); scripts stay restricted to `'self'`. Under WebView2 it is open whether the hook
   sees navigations in the sub-frame (wry subscribes there only to `NavigationStarting`); stage 6 checks this.

**Shell**
- The main window is created only when the core is ready (`"create": false` in `tauri.conf.json`); this way the UI cannot send commands before `AppCore` is running.
- `devCsp` does not apply in development mode (the WebView loads the `devUrl` directly). CSP errors only show up in the release or e2e build; the desktop test records CSP violations and fails on them.
- Minimal Tauri capabilities: no file system, shell or HTTP plugins for the frontend.
  The notification plugin (stage 9) is used only by the shell; the UI has no permission
  for it. The new-mail popup (stage 18, 0051) is a window of the app, see below; it adds no
  network connection (the sender's picture is the helper's `data:` JPEG of stage 17). Likewise the updater plugin (stage 15, 0037): no capability grants `updater:*`, the
  UI uses the app's own commands `getUpdateInfo`, `setUpdateAutoCheck`, `checkForUpdates`,
  `downloadUpdate`, `installUpdate`. Whether to ask is decided by `mail-engine/src/updates.rs`
  (switch, one-time question, daily throttle), the shell only plugs in the plugin as `UpdateSource`.
  The private update key is only in the keychain of the build Mac
  (`org.vicemail.updater`, `docs/RELEASING.md`), the public one in `tauri.conf.json`. The default for `mailto:` is queried and set by the shell on macOS via `osascript` with a fixed
  script (Launch Services without `unsafe`, 0020).
- Strict Content Security Policy for the app itself, no remote sources.
- The embedded WebDriver for tests is only contained in test builds, never in shipped ones
  (Cargo feature `e2e`; a test checks that the release binary does not contain it, 0005).
- Tauri init scripts are never injected for all frames.
- `unsafe` is forbidden everywhere (`forbid`), except in `crates/webview2-guard` (`deny` with individually
  justified `#[allow(unsafe_code)]` and `// SAFETY:`); `scripts/check.sh` checks this by searching.
- New windows (`target=_blank`, `window.open`) are always rejected by the shell, as are downloads by the
  WebView itself (context menu "Download Linked File", "Save Image As"; stage 15).
- Otherwise the app never opens web pages itself, only after a click and a check, via the system browser.
  The only exception (stage 14, 0042): "Sign in with Microsoft/Google" and "Sign in again" open
  the provider's fixed sign-in address in the system browser, via the same opener as links (never in
  the WebView, no window of its own). The answer comes via the core's loopback listener, not
  via the UI.
- Windows are opened only by the shell (stage 8, 0025): the main window, one mail window per mail
  (`openMessageWindow`, label `message-<account>-<message>`, page `index.html?window=message&…`) and
  one composer window per draft (`openComposerWindow`, `composer-<draft>`,
  `index.html?window=composer&draftId=…`). Every window is created via `windows::guard` and
  `windows::finish`: the same navigation hook, the same rejection of new windows, on Windows
  the same WebView2 frame guard. The capability `default` applies to `main`, `message-*` and
  `composer-*`, so each of these windows has exactly the rights of the main window; other labels
  have none. Which window calls a command (layout per
  window) is taken by the shell from the calling `WebviewWindow`, never from an argument.
- The new-mail popup (stage 18, 0051): label `new-mail-popup`, its own page `popup.html` (a
  separate entry of the UI build), built through `windows::guard`/`finish` like every window,
  created with the first card and destroyed with the last one (since stage 21 also while the main
  window is closed into the background; not while the app quits). No
  decorations, always on top, not in the taskbar or the Window menu, on every Space or desktop,
  at the bottom right of the work area of the monitor showing the main window. It never takes the
  focus: `focused(false)`, `focusable(false)`, `accept_first_mouse(true)` (macOS: never key, the
  app is not activated when it appears; Windows: `WS_EX_NOACTIVATE`, created visible because
  only the creation shows a window without activating it). Its rights are minimal: Tauri does
  not govern the app's own commands by capability (the app has no ACL manifest), so the shell's
  invoke handler refuses every command from this window except `POPUP_COMMANDS`
  (`get_new_mail_popup`, `read_from_popup`, `delete_from_popup`, `undo_popup_delete`,
  `dismiss_popup`, `open_inbox_from_popup`, `fit_new_mail_popup`) before it runs; its capability
  `new-mail-popup` grants only `core:event:allow-listen`/`allow-unlisten`; the core's events are
  not emitted to it (`events::hears_core_events`), it only receives `new-mail-popup-changed`.
- The main window comes and goes (stage 21, 0069): with "keep running" (default) closing it
  destroys the window and its web view and the app keeps running; Tauri's exit request when the
  last window is gone (`ExitRequested` without a code) is held back, an exit with a code
  (`AppHandle::exit`, the tray's Quit, the updater's restart) and macOS's own Quit end the app.
  The first close asks the page once (`closeToBackgroundAsked`, answered by `closeMainWindow`).
  `lifecycle::show_main_window` builds a new main window (on the async runtime; WebView2 deadlocks
  when a window is built from a main-thread handler) where the last one was: Dock click
  (`RunEvent::Reopen`), the second start (single-instance plugin, Windows), the tray icon, a
  notification, "Read" in the popup, a `mailto:` link. Requests for the main window
  (`openMessageRequested`, `openInboxRequested`, `composeRequested`) that arrive while its page
  loads wait in the shell and are emitted once the page says it listens (`mainWindowReady`).
  The new-mail popup no longer closes with the main window; it opens while the main window is
  closed and not while the app quits. A start with `--background` (the login item, 0070) opens no
  window.
- The tray icon (stage 21, 0069): Windows always (the way back to a closed window), macOS unless
  switched off (`menuBarIcon`); unread count (macOS: the item's title; Windows: drawn into the
  icon, `badge::on_icon`, and in the tooltip), menu Open, New Message, Pause Notifications (1 h,
  until 08:00 tomorrow, resume), Quit. The texts come from the UI (`setDesktopTexts`) and are kept
  by the core. No capability: the UI cannot reach the tray.
- The counter (stage 9, redrawn in stage 21, 0068, digits from a font since stage 22, 0080):
  macOS `NSDockTile.badgeLabel` on the app (current without a window); Windows the taskbar
  overlay drawn at 16 logical pixels for the window's scale (`badge::overlay`, "99+") and drawn
  again on `ScaleFactorChanged`; every window's icon is the app icon's own entry for the taskbar
  size of its scale (24 logical pixels, `Image::from_app_icon_resource`). The digits are glyphs of
  the embedded Selawik subset, auto-hinted by skrifa (strongest mode: zones, stem edges and
  widths on whole pixels) and filled by `badge.rs`'s own scanline rasteriser; every count with as
  many glyphs gets the same size, the largest that stands on solid red with a rim. `overlay` and
  `on_icon` return `BadgeError` (never with the embedded font; the callers log it). The installer
  hook tells the shell after install and update that the icon changed (`SHChangeNotify`), so
  pinned shortcuts drop the icon Windows cached for the replaced exe.
- The Windows 11 look (stage 22, 0079, `backdrop.rs`): when a window is built on Windows 11
  (build 22000+) with transparency effects on and no contrast theme (read from the registry),
  it gets Tauri's `Mica` effect (window-vibrancy; follows the window's dark mode, which
  `window_theme` keeps on the app's choice), so the native title bar is drawn in the material.
  The main window is also built transparent (`transparent(true)`); `get_window_backdrop` tells
  its page `mica`, the UI sets `data-backdrop` and `app.css` leaves the page and its chrome
  (`data-chrome`: frame, icon rail, docked folder column) transparent, only under
  `prefers-reduced-transparency: no-preference` and `forced-colors: none`. Content surfaces stay
  opaque. Other windows: Mica in the title bar only. Native caption buttons stay (snap layouts).
  No `unsafe`; elsewhere (macOS, Windows 10, settings off) nothing changes.
- Start with the system (stage 21, 0070): `autostart.rs`, a launch agent of the user
  (`~/Library/LaunchAgents/<id>.plist`, `RunAtLoad`, the executable with `--background`) or the
  Run key value `<product name>` (plus Task Manager's `StartupApproved` state) on Windows; the
  system's registration is what the switch shows; development builds do not register,
  desktop-test builds write into the test directory.
- `.eml` files (stage 9, 0027) appear in mail windows with the label `message-file-<ID>`
  (`index.html?window=eml&fileId=<ID>`, ID 128 bits random): the same rights and protection rules
  as every mail window. The app registers for `.eml` (`bundle.fileAssociations`; macOS with
  rank "Alternate", Mail stays the default; on Windows the installer enters the association).
  Files arrive via `RunEvent::Opened` (macOS), launch arguments and the second launch
  (`tauri-plugin-single-instance`, Windows) or the core's open dialog.
- The source window of a message (stage 19, 0055): label `source-<account>-<message>`, page
  `index.html?window=source&…`, built through `windows::guard`/`finish` like every window. Its own
  capability `source-window` grants only `core:window:allow-close`: no events, no dragging, no
  plugin. The page reads its preferences once and shows the raw message as one text node (control
  and direction characters as visible symbols, at most 1 MiB, "Save as .eml" for the whole).
- Desktop-test builds only: `e2e_dialog_answers` queues the paths the next file dialogs return
  (stage 19), since WebDriver cannot drive the system's dialogs.

**Secrets**
- Passwords, tokens and API keys only in the system keychain. A type `Secret<T>` prevents
  them from ending up in logs or error messages; a test checks this.
- Stage 14 (0042): OAuth refresh tokens likewise (entry `oauth:<provider>:<id>`); access tokens only
  in memory. The diagnostics export contains neither (test). Client IDs are not secrets.
- Stage 14 (0041, addendum 1): API tokens of JMAP accounts (Fastmail) like passwords (entry
  `token:<id>`), sent only as `Authorization: Bearer` to the account's own JMAP server, never logged,
  not in the diagnostics export (test), not in the settings sync (the record only says
  `"signIn": "apiToken"`).
- Removing an account deletes the keychain entry, local data and files of this account; for Google
  the refresh token is revoked first.

**Files**
- File names of attachments are sanitised: no path separators, no reserved Windows names,
  no control characters, no direction-reversal characters. Writing happens only to the location the
  user chooses in the system dialog.
- Subjects and display names (headers via IMAP in `mail-imap`, whole mails, contacts,
  invitations and rule matching in `mail-content`) lose the same direction characters
  (U+202A–202E, U+2066–2069, U+200E, U+200F, U+061C; `mail_model::is_bidi_control`), so that
  `Invoice\u{202E}fdp.exe` does not appear as `Invoiceexe.pdf` (security review C-8).
  Headers stored before keep them until the local state is rebuilt (new UIDVALIDITY, account set up again); no version had been published yet.
- Every file the app writes from an attachment (Open, Save, Save all) carries
  the system's mark of origin: on Windows the `Zone.Identifier` (zone 3), on macOS
  `com.apple.quarantine` (stage 15, via `/usr/bin/xattr` with fixed arguments), set
  while the file is still writable. This way SmartScreen and Gatekeeper check it as they check a
  download, including an app from an attached archive.
- "Save all" (stage 16, 0036): a folder from the system's folder dialog; each file under
  its sanitised name, claimed with `create_new`: an existing file is never replaced, a
  taken name becomes `Name (2).ext` and so on; on Windows with a mark of origin like every save.

**Attachment preview (stage 16, 0036)**
- The core decodes, the WebView only shows what the core itself generated: images (PNG, JPEG,
  GIF, WebP, recognised by their bytes) decoded in Rust (at most 64 megapixels and, checked from the header already, at most 192 MiB decoded, security review C-9), scaled down to
  2048 px and re-encoded (PNG with transparency, otherwise JPEG), PDF pages rasterised to PNG one by one with hayro
  (at most 200 pages), text decoded and truncated to 512 KiB, shown as text.
  At most 32 MiB input. HTML, XHTML, XML and SVG never, even if only the type or name says so.
- Images and PDF are decoded not by the app process but by the helper `mail-mcp` in preview mode
  (`--render-preview`, security review C-2, 0043): file via stdin, answer as JSON via
  stdout; the core ends it after 20 s or above 512 MiB of resident memory (polled every
  50 ms with `ps` or `tasklist` respectively). An abort is `preview_too_large`, a crash
  `preview_damaged`; the app keeps running. Without the helper a release build rejects these previews.
- No window of its own and no change to CSP, capabilities or navigation hook: the dialog
  in the reading pane shows `data:` images (covered by `img-src data:`) and text. The PDF viewer
  of the WebView is not used (it would need `object-src`/`frame-src data:` and runs PDF scripts under
  WebView2).
- Typed errors `preview_unsupported`, `preview_too_large`, `preview_damaged`; a panic
  of a decoder becomes `preview_damaged`.
- `.eml` files (0027) are read only by the core, only from the path the system passes or the user
  chooses in the open dialog, at most `MAX_EML_FILE_BYTES` (64 MiB, checked before and while reading);
  typed errors `eml_too_large`, `eml_invalid`, `eml_unreadable`. They are not
  imported: no account, no database row, no search index; the bytes stay in memory
  as long as their window is open (at most 16 files). They are displayed via the same
  pipeline as HTML from mails (sanitising, mandatory CSP, sandbox frame, link placeholders with
  their own render token, attachments via the core).

**Thunderbird takeover (stage 19, 0057)**
- The core reads only `profiles.ini`, the chosen profile's `prefs.js` (at most 16 MiB), the
  signature files its identities name (at most 64 KiB, text) and the mbox files under "Local Folders"
  (no links followed). It never opens `logins.json` or `key4.db`; no password is read. The parsed
  profile stays in memory under a random token; the UI sees no path. Accounts go through the normal
  setup (password typed by the user, read-only start); a plain connection in Thunderbird becomes
  STARTTLS.

**vCard (stage 19, 0056)**
- Read and written by the app's own reader in `mail-content` (no new dependency). Pictures only
  from embedded bytes (never links), PNG/JPEG/GIF/WebP by their bytes, decoded and encoded again in
  the helper like a chosen contact picture (centre square, 256 × 256 JPEG).

**Contact pictures (stage 17, 0050)**
- The user picks a file in the core's system dialog or drops it on the window (only paths the
  shell saw dropped); the core reads at most 20 MiB. The helper (`mail-mcp --contact-picture
  source`) decodes it with the preview's limits and encodes the whole image again (JPEG, at most
  1024 px) for the cropper; the core keeps it in memory under a token (15 minutes, four at most).
  `--contact-picture crop <x> <y> <size>` cuts the chosen square out of that image and encodes a
  256 × 256 JPEG of at most 40 KiB (quality 85 down to 35). The answer must be exactly
  `data:image/jpeg;base64,…` of a JPEG within the size. Same watchdog (20 s, 512 MiB); typed
  errors `picture_too_large`, `picture_unsupported`, `picture_damaged`.
- Stored in the database (`contact_picture`), shown as `data:` (covered by `img-src data:`).
  A picture from the settings sync is shown only after this device's helper encoded it again.
  No picture from the network (no Gravatar, BIMI or favicons).

**Images in mails (security review C-4, 0046)**
- `render_html` shows an image (inline part, approved external image, `data:` URL in the HTML)
  only as its re-encoding from `RenderOptions::images` (key: the original bytes);
  if that is missing, the image is left out and is listed in `RenderedHtml::pending_images`. Size and
  format checks (8 MiB per image, 32 MiB per mail, PNG/JPEG/GIF/WebP by their bytes) come first.
- The core (`reading/images.rs`) renders with what it already knows, sends the missing images
  and the external ones just loaded in one batch to the helper (`mail-mcp --reencode-images`:
  per image a 4-byte length and the bytes via stdin, one JSON answer via stdout; at most 512
  images and 64 MiB per batch) and renders a second time. Watchdog as for the
  attachment preview (20 s, 512 MiB). Only `data:image/(png|jpeg|gif);base64,…` from the answer goes
  into a document.
- In the helper (`mail-content`, `reencode.rs`): spread over up to eight cores, images decoded
  at the same time together at most 384 MiB (estimated from the header), limits as for the
  attachment preview (64 megapixels, 192 MiB decoded, 2048 px). Still images as PNG (with
  transparency) or JPEG; animated GIF and WebP as animated GIF (at most 300 frames,
  12 Mi pixels in total, 16 MiB, canvas up to 2048 px; otherwise the first frame).
- Rejected images and failed batches are shown by the mail like any image that cannot be
  shown (element without a source), never with the original bytes.
- Results and rejections are remembered by the core in memory (key from two SipHash values
  with random keys, at most 64 MiB and 4096 entries, least recently used evicted
  first); a mail pays the cost on its first display (newsletter with 32 images: about 20 ms in a
  release build). A helper that could not start is tried again the next time.
- Without the helper: tests and development builds re-encode in the process with the same code; a
  release build always passes the path and does not show the images if the file is missing.
- The editor shows inline images of drafts (also of drafts of other programs) and
  signature images re-encoded as well (`Reading::editor_images`); draft and signature keep
  and send their own bytes.

**Allowed network connections**
- Mail servers the user has set up, also for bodies and attachments (stage 3): per account a
  separate read connection besides sync and IDLE, always read-only (EXAMINE, `BODY.PEEK[…]`);
  for the sync account one more for the settings sync (stage 11), which writes only to the sync folder
- JMAP accounts (stage 14, 0041): the session at `https://<host>[:<port>]/.well-known/jmap` of the
  configured server (redirects only to the same host or its subdomains, at most
  three) and the addresses this session names (API, download, upload, EventSource), HTTPS only.
  Per account the connections as with IMAP (sync, push, reading pane, outbox), each its own
  HTTP client without proxy, cookies, referrer and retries
- During setup, only after clicking "search online": DNS SRV queries (RFC 6186) via the
  system DNS servers for the domain of the address, then HTTPS (port 443) for the domain's own
  autoconfiguration (`autoconfig.<domain>`, `<domain>/.well-known/autoconfig/`). Redirects only to
  the same domain or its subdomains, at most three, without cookies, referrer and proxy, 128 KiB
  at most. No Mozilla ISPDB, no other third-party directory. Servers found are shown before the
  confirmation (SRV and autoconfig are not DNSSEC-validated). A server outside the
  domain of the address (neither the domain nor a subdomain of it, compared in ASCII/IDNA form ignoring
  case and a trailing dot) or given as an IP address gets a warning of its own
  (`DiscoveryResult.warnings`, RFC 6186 section 6, security review N-4,
  `discovery/warning.rs`); the servers of the built-in provider table are exempt, and its
  results never carry a warning. MX is not queried. From stage 14 (0041) additionally
  an HTTPS GET without credentials to `<domain>/.well-known/jmap` (same rules for
  redirects), to offer a JMAP server.
- External images of a mail, only after approval ("load once" or "always for this sender";
  for an opened `.eml` file only "load once", 0027).
  The core loads them itself (`mail-engine/src/reading/remote.rs`): reqwest over rustls with the
  system's certificate verification, HTTP/1.1, without cookies, referrer, proxy and retries, with a
  neutral identifier (`Mozilla/5.0`), only `http`/`https`, redirects by hand (at most three,
  again only `http`/`https`), 5 s connection setup, 15 s per image, 30 s per mail, at most 200
  addresses, 8 MiB per image and 32 MiB per mail. **Never to private, loopback or link-local addresses**
  (nor to CGNAT, multicast, reserved and documentation networks, IPv4 embedded in IPv6 included):
  IP literals are checked before the request, names after resolution, and the connection uses only
  the checked addresses. An exception, for loopback only, exists solely in tests
  (`RemoteConfig::allow_loopback`; in the desktop test build `MAIL_APP_E2E_LOOPBACK_IMAGES=1`, only with
  feature `e2e`). Limit: the `From` address can be forged; whoever forges an approved sender
  gets their images loaded, nothing more (tracking pixels and known tracking services stay blocked
  while `reading.block_trackers` is on, stage 20)
- Links and attachments are opened not by the app but by the system (browser, mail program, default app),
  only after a click and a check, links only `http`, `https`, `mailto`
- AI endpoints the user has set up and approved (stage 13, 0024): only the base URL
  of a profile, only with AI turned on, to a cloud provider only after consent and only with
  mails from accounts with cloud approval; `http` only to local addresses, no redirects,
  proxies or retries
- The MCP server only accepts local connections (helper program via stdio, to the app via a
  Unix socket or a named pipe, never TCP; 0015) and opens none itself
- Unsubscribing (stage 9, 0020), only after a click: one-click per RFC 8058 as HTTPS POST
  (`List-Unsubscribe=One-Click`) to the `https` address from `List-Unsubscribe`, only if
  `List-Unsubscribe-Post` offers it; client as for external images (shared TLS configuration,
  without cookies, referrer, proxy and retries, neutral identifier, never to private, loopback or
  link-local addresses), no redirects, 5 s setup, 15 s total. A `mailto:` unsubscribe is
  a draft that the user sends; a web unsubscribe is opened only by the system browser
- Update check (stage 15, 0037), **only with consent**: automatically only if the user has
  turned on "Check for updates automatically" (default off; asked once, in the "Optional" step
  of the first start or, if that was skipped, after the first account, 0044),
  at most once a day; otherwise only after clicking "Check now". The only target is the
  endpoint from `tauri.conf.json` (`plugins.updater.endpoints`,
  `https://github.com/othervice/vice-mail/releases/latest/download/latest.json`), plus, for the
  redirect and the download, GitHub's file hosts `objects.githubusercontent.com` and
  `release-assets.githubusercontent.com`. HTTPS only, redirects only to these hosts (at most
  five), on `github.com` only paths under the endpoint's `/<owner>/<repo>/releases/` (stage 15),
  a download address outside them is rejected before any connection. Client as for
  external images: shared TLS configuration, without cookies, referrer, proxy and retries,
  neutral identifier, no app version and no identifier in the request. Downloading and
  installing only happen after a click; the file must be signed with the update key and for exactly the
  announced version. Time limits: manifest 30 s, download 15 minutes in total;
  the download stops as soon as more than 300 MiB are announced or have arrived
  (security review N-3). Other endpoints only in tests and in the desktop test build
  (`MAIL_APP_E2E_UPDATE_ENDPOINT`), HTTP only to loopback endpoints
- Sign-in with Microsoft and Google (stage 14, 0042), only after clicking "Sign in with Microsoft/Google"
  or "Sign in again" and for such accounts: the system browser opens the
  sign-in page (`login.microsoftonline.com`, `accounts.google.com`); the core talks to the
  token endpoint (`login.microsoftonline.com/common/oauth2/v2.0/token`,
  `oauth2.googleapis.com/token`) when signing in and about hourly to renew while the account
  is running, and to `oauth2.googleapis.com/revoke` when a Google account is removed. Client as for
  account discovery (shared TLS configuration, HTTPS, without redirects, cookies, proxy and
  retries, neutral identifier, 64 KiB response). During sign-in the core listens on
  `127.0.0.1` (and `::1`) on a free port, only for the browser's redirect back; HTTP to
  other endpoints only in tests to loopback addresses
- The operating system's certificate verification may query revocation information
  (OCSP, CRL) from the issuing authority for public certificates. This happens in the system, not in the app.

Nothing else. No telemetry, no crash reports, no queries of third-party directories without a click.

## Settings sync

The mail server syncs mails, folders, flags and drafts. The settings sync only syncs
what only the app knows, and uses the mailbox itself as storage for it.

- **Storage:** a dedicated folder in an account chosen by the user ("sync account"), not subscribed,
  so that most other mail programs do not show it. Every record is one message.
- **Encryption:** format `age`. On setup a random key is created; it is itself stored in the folder,
  encrypted with the passphrase. Records are encrypted to this key. Headers carry only an
  opaque ID and a version; kind and content are in the encrypted part and are checked
  against the headers on reading.
- **Changing:** append the new version, delete the old one. If two versions exist, the
  merge rule of the data kind decides, the result is written, the losers are deleted.
- **Versions:** hybrid logical clock per record (time, counter, device ID).
- **Deleting:** tombstone record, kept for 180 days, so that devices absent for a long time learn about it.
- **Rules per data kind:**

| Data kind | Rule |
|---|---|
| Settings | per key the newer version wins |
| Signatures, templates, rules, views | per record the newer version wins; editing beats simultaneous deletion |
| Snooze, pins, tasks, reading list, reminders without a reply (`followUp`, stage 20, 0059) | per mail the newer version wins; whether a reminder came back is decided by each device |
| Pinned attachments | per attachment (account UUID, Message-ID, index) the newer version wins (stage 16, 0036) |
| Folder settings (favourite, colour), blocked senders | per folder (account UUID and path) or address respectively the newer version wins (stage 16, 0033) |
| Sender rules, image approvals, contact names and notes, contact pictures, devices | per key the newer version wins; contact pictures (stage 17, 0050) as their own kind `contactPicture`, shown only after this device's helper encoded them again |
| Contact groups (stage 19, 0056) | per group the newer version wins; editing beats simultaneous deletion; the receiving device checks name, search words and member keys before applying |
| Identities, signatures (separate record per identity), account groups | like signatures |
| Account list | per field the newer version wins; a removed account is not silently deleted on other devices, instead the user is asked; other servers (host, port, security, user name) for an existing account are not taken over but wait for the user (0040); the protocol (field `protocol`, only for JMAP accounts) and the kind of sign-in (field `signIn`, only with an API token) apply only when an account is adopted, never to an existing one (0041) |

- **New device:** set up the sync account, enter the passphrase, preview of the data to be taken over,
  merge. Adopted accounts ask for their password one by one.
- **Passphrase forgotten:** nothing is lost, every device has its data locally. Reset the sync,
  new key, upload again from this device.
- **Visible to the mail server:** number, size and times of the records. Nothing more.
- **Transport exchangeable:** a trait `SettingsTransport` (list, write, delete). The first
  implementation is the IMAP folder; a self-hosted server or a file folder may follow.
- **Implementation (stage 11, `docs/decisions/0022-einstellungs-sync-integration.md`):**
  - Folder `Settings-Sync` (at the top or under `INBOX`), not subscribed, hidden from the folder list of the
    mail sync; per version one message with `X-Settings-Sync-Id` and
    `X-Settings-Sync-Version`, body age ASCII armor. The sync never touches messages without both
    headers.
  - Capture in one hook: triggers on every syncable table mark the record,
    `Store::write` has the engine version it before the commit (`SyncCapture`).
  - Separate connection that may write only to the sync folder (check of every command line in the
    adapter, `FolderScope`), also with `writes_enabled = false` (0014). Its failure does not affect reading
    and sending.
  - Passes after local changes, after every mail pass of the sync account and at the
    poll interval; unchanged folder state (UIDVALIDITY, UIDNEXT, EXISTS, HIGHESTMODSEQ) and
    nothing open locally means: nothing to do. A full check once a day.
  - Limits per pass (security review F2, `FetchLimits`), by the listed sizes
    (`RFC822.SIZE`), before anything is read: entries over 2 MiB are never downloaded, never
    deleted and never count as a version (reported like rejected ones); at most 5,000 entries and
    64 MiB per pass, a record entirely or not at all. What does not fit waits as a whole for
    the next pass, which then runs even with an unchanged folder.
  - Second device: the sync account set up beforehand and its default identity adopt
    the IDs from the folder (same address, never reconciled); adopted accounts start
    read-only and without a password, with a new, random keychain name.
  - Servers are a boundary for credentials (security review F1, 0040): if a record names
    other servers for an existing account, the device keeps its servers and its password; the
    account shows "Server settings changed on another device". "Use the new servers" first deletes
    the password, turns off write access and then writes the servers; the account
    then asks for the password. "Keep the current servers" leaves everything as it is.
  - Account removed elsewhere: the user is asked; until the answer, the deletions of its
    identities, snoozes etc. wait too; "keep" renews the account and these records.

## AI

Two ways, both off until the user turns them on (`docs/decisions/0010-ki-ueber-mcp.md`):

**MCP server (stage 12, main way).** The app offers a local MCP server. Any MCP-capable
AI client (Claude Desktop, Claude Code, Cursor, LM Studio …) can search approved accounts,
read threads and create drafts. For this the app contains no provider and no key.
- Local only (0015): the client starts the bundled helper program `mail-mcp` via stdio; it talks
  to the running app via a Unix socket (0600, directory 0700, check of the peer UID) or a
  Windows named pipe for the user only, with HMAC challenge-response per client. Never TCP (no
  DNS rebinding, no access by other local processes via a port).
- Per account: invisible, read, read and drafts. Default: invisible.
- No tool sends, deletes, moves, flags or changes settings.
- Tools return sanitised text (no raw HTML, no external images, attachments only by name),
  marked as untrusted data, with size and quantity limits. What counts as text is what
  the reading pane shows: the visible text of the HTML part, only without HTML the text part (the same for MCP and
  the built-in AI, 0021). Bodies are fetched only with `BODY.PEEK`; nothing is marked as read.
- Integration (0021): `AppCore` implements `MailAccess` (exactly the approved accounts, trash and
  spam only when the mailbox is given, also when reading by ID: a mail that is only there counts as
  non-existent without `mailbox_id`, a thread leaves it out, 0045), drafts via the drafts from stage 5 (without write access only locally),
  the helper program is in the bundle as a Tauri sidecar (`Contents/MacOS/mail-mcp`, next to the app on Windows).
- Every access is visible in the app; clients can be revoked individually, emergency stop for all.
- Limit, named honestly: the client has its own tools, and mail contents can manipulate it with
  injected instructions. The app limits what it gets and may do; what a
  cloud client does with mails it has read is outside the app.

**AI in the app (stage 13, optional).** For features in the UI (summarising,
reply suggestions, writing help, search in natural language) a trait `AiProvider` with one
implementation over the OpenAI-compatible chat interface with a freely selectable base URL (Ollama,
LM Studio, llama.cpp server, OpenRouter, OpenAI) and a second one for the Anthropic Messages API: over
the compatible interface, refusals cannot be detected (0019). Crate `mail-ai`, depends only on
`mail-model` and `mail-content`; the TLS configuration is passed in from `mail_imap::tls`.
- Mail contents are data, not instructions: fixed system text in the code, mail text clearly delimited in the user part.
- The built-in model gets no tools. So structurally it cannot send, delete or fetch anything.
- Outputs are displayed as text. Images from AI outputs are never loaded, links go through
  the same check as links in mails (0028: the core detects them in its own result as in
  plain-text mails, `segments` and `linkToken` per result, `describeLink`/`openLink`).
- Only what the request needs is sent: the current thread without duplicate quotes and without
  attachments. Never the whole mailbox without explicit confirmation stating the amount.

Wiring (0024): before every request `AppCore` (module `mail-engine/src/ai/`) checks the switch,
the profile of the function, consent (cloud) and the account's cloud approval, reads the key from the
keychain and reports every request to the provider beforehand as the event `aiRequest` (mails,
bytes, accounts, host, model), then progress and end; cancellable via the request ID. A
log holds only metadata. A key belongs to the server it was entered for
(V-02): if a profile is saved with a base URL of another origin (scheme, host, port) without a new
key, the core first deletes the stored key from the keychain
(if that fails, nothing is saved); the profile then has no key and reports
`keyRemovedOnServerChange` until it is saved again, and the settings ask for the
key. Changes of model and path keep it. The model list and the connection test work only
with the saved profile; every request reads profile and key together under the lock of
the settings. Results are checked data: statements only with references to
the mails sent to the provider, reply suggestions and writing help as text. A reply suggestion becomes a normal draft only on the
user's click (stage 5). Notices of open replies are rules
without AI. Every saved change of the settings reports `aiSettingsChanged` with the
availability afterwards to all windows (0027). The sources of an answer are named and opened by the
UI via `getMessagesByIds` (at most 200 IDs, unknown ones are missing). Before consent,
the consent step shows the exact size from a dry run (`aiPrepareTransfer`, 0028): the same
preparation as the request, without consent, without event, without log and without a connection
to the provider; for questions the first of the two requests. Whether the AI panel is expanded or collapsed
is stored in the core's UI settings (`aiPanel`, only on this device).

## Keyboard shortcuts

Stage 8. All shortcuts come from the action registry (`app/src/features/actions/registry.ts`,
actions and app commands with default shortcuts per platform). The user's changes are stored in
`app/src/stores/shortcuts.ts` (per platform the whole list of a command); every key press reads
the current binding, so a change takes effect immediately. Rules (`app/src/features/keyboard/bindings.ts`):

- **Keyboard layouts:** a shortcut stands for the character the key produces (`event.key`), not
  for its position (`event.code`). "#" is "#" on German and US keyboards, wherever the key is.
  Letters ignore upper/lower case, but not Shift; other characters ignore
  Shift (whether they need it depends on the layout). If a shortcut ever needs the position of a
  key, it gets a separate field for it instead of reinterpreting `key`.
- **Special keys:** `mod` is Cmd on macOS and Ctrl on Windows. Option/Alt combinations cannot
  be assigned (on many layouts they produce characters, AltGr reports Ctrl+Alt).
- **Not assignable:** what the system or every text field claims (macOS Cmd+Q/W/H/M/,/Space/`/Tab,
  everywhere copy, paste, cut; Windows Ctrl+Esc, Ctrl+Tab), the fixed keys for
  moving (arrows, j/k, Home/End, Page Up/Down, Enter, Esc, Tab, also with Shift) and the
  shortcuts outside the registry (Cmd/Ctrl+P prints, Cmd/Ctrl+Enter sends).
- **Double assignment:** is rejected and names the command that has the shortcut. Exceptions are commands
  that are never available at the same time (Delete/Delete permanently, Junk/Not junk).
- **Default:** e archives, # deletes, r/a/f reply and forward, c composes, / and
  Cmd/Ctrl+F search, j/k and arrows move, Enter opens, Esc closes, Cmd/Ctrl+K opens the
  command palette, ? the overview, Cmd/Ctrl+1 to 8 the views of the icon bar, Cmd/Ctrl+U the
  source of the message (stage 19). On layouts on
  which digits need Shift (AZERTY), Cmd/Ctrl+Shift+digit applies, because Shift does not count
  for characters.

The changes are stored in the core (0025): `getUiPreferences` / `setUiPreferences` with the
changed commands per platform (`{ command, shortcuts: [{ key, shift, mod }] }`, at most 100
commands with at most 4 shortcuts each), the text size (80 to 150) and the at most 5 most recently
used commands of the palette; every change goes as `uiPreferencesChanged` to all windows.
Shortcuts and text size are synced by the settings sync (one key per platform), the history
of the palette stays on the device. The layout (reading pane on the right, at the bottom or instead of the list,
density, sidebar, the sidebar's width and, since stage 18, the list's width beside the reading pane and
its height above it) is stored by `setWindowLayout` per window kind on the device. The core checks shape
and limits; which commands exist and which shortcuts are assignable is decided by the UI.
Until the UI has been switched over, these values are still kept in the marked fallback store
`app/src/features/keyboard/localFallback.ts`.

## Tests

- **Unit:** parsers, normalisation, threads, search language, merge rules, categories.
- **Integration:** `AppCore` against the local test server, without WebView. This is where the weight lies.
- **Fault injection:** a small TCP proxy in the test cuts connections at specific points
  (during the sync, after the transmission when sending).
- **UI:** Vitest for components; WebdriverIO against the real app for a few end-to-end flows.
- **Security:** corpus of hostile HTML mails and file names; counter for outgoing connections.
  The counter counts TCP connections, not just HTTP requests (preconnect opens only a connection).
  Every "zero accesses" proof has a positive control that shows that the counter counts.
- **Performance:** generated mailboxes with 100,000 and 500,000 mails, results in `docs/testing/performance.md`.

## Decisions

| Decision | Reason | Way back |
|---|---|---|
| Logo in blue (0068), approved by the human on 2026-11-25; icons from one script (`scripts/icons.sh`) | the GitHub blues of the colour scheme (0053); every size from its own source, so a new artwork is one command | swap the colours back in `make_svgs.py` and `Brand.tsx`, run the script |
| The counter drawn for the display scale, window icon from the icon's own entry (0068) | Windows shows the overlay at 16 logical pixels; a 32 px image shrunk to 16 blurs the digits | the stage 9 overlay (32 px, one size) |
| The counter's digits from an embedded font (Selawik subset, OFL), auto-hinted at the exact pixel size; installer refreshes the shell's icons (0080) | the bitmap fonts of 0068 were sharp but blocky ("pixelated"); pinned shortcuts kept the cached orange icon after the update | the bitmap fonts of 0068 (`badge.rs` of stage 21) |
| Mica on Windows 11: title bar of every window, behind the main window's chrome; native caption buttons (0079) | the human chose the Windows 11 look; Tauri's effects need no `unsafe`; content stays on opaque surfaces for contrast | `backdrop::choose` returns `None`: no effect, opaque window, as in 0.1.2 |
| Closing the main window keeps the app running, web view destroyed, tray icon (0069) | the human chose it after the test of 0.1.1; no idle web view in the background (0048) | switch "keep running" off; without it the app quits with its last window as before |
| Own login item (launch agent, Run key) instead of `tauri-plugin-autostart` or `SMAppService` (0070) | starts hidden with `--background`, no new dependency, testable with a fake | the plugin, or `SMAppService` once a maintained binding exists |
| Trackers are counted and never loaded; links lose tracking parameters; redirects are not resolved (0058) | the user sees what the mail tried; the request for an approved image never carries tracking data; resolving a redirect would call the tracking service | turn off `reading.block_trackers` and `reading.clean_links` |
| Reminders without a reply are local records like snoozes; any message of the thread from someone else ends them (0059) | no write on the server, works across folders and devices, stage 7 threading is exact | remove the `follow_up` table and the composer option |
| The "Attachments" view lists the attachments of stored bodies (index written with the search index), not of every BODYSTRUCTURE (0060) | same index as the reading pane, no extra traffic in the sync; preloading decides how far back it reaches | read BODYSTRUCTURE in the header sync and write rows there |
| Storage quota over the read-only connection, kept 15 minutes; cleanup via the search filter `larger:` (0060) | RFC 9208 / RFC 9425, nothing written; one cleanup path for both protocols | drop the block; the `over_quota` message stays |
| Sync via a folder in the mailbox instead of our own server | only a few kilobytes to sync; no server, no login, no pairing, usable by everyone | second transport via `SettingsTransport` |
| No database encryption in version 1 | FileVault and BitLocker protect the disk; SQLCipher costs speed and makes Windows builds harder | rusqlite can switch on SQLCipher via a feature |
| rusqlite instead of SQLx | one process, one writer; synchronous is simpler and faster | storage access is encapsulated behind `mail-store` |
| AI only via interfaces | no model download, no GPU builds per platform; Ollama covers "local" | embedded runtime as a further `AiProvider` implementation |
| AI via MCP first (0010) | any AI fits, no providers and keys in the app, little code | built-in AI (stage 13) supplements it, MCP stays |
| IMAP first, Microsoft and Gmail API later | your own accounts run over IMAP; OAuth providers need registrations | `MailBackend` trait |
| JMAP behind the IMAP-shaped traits with its own UID numbering (0041) | sync, outbox, conflicts, reading pane and search stay one logic for both protocols | a separate JMAP sync in the engine via `Email/changes` |
| Microsoft and Google via IMAP/SMTP with OAuth instead of Graph or the Gmail API respectively (0042) | existing adapter, the Gmail specifics from block A stay; the Gmail API would have the same "restricted scope" | a separate adapter behind `MailBackend` |
| Attachment preview in the core instead of in the WebView's PDF viewer (0036) | the WebView only sees re-encoded images and text; no CSP loosening, no PDF scripts | turn off the PDF preview (`preview_unsupported`), opening in the system remains |
| Images in mails only re-encoded, in the helper (0046) | the system's image decoders see no bytes from mails (C-4); one batch per rendering, cache in memory, about 20 ms per newsletter | re-encoding in the app process (same code, only without isolation) |
| New mail in an app-owned popup window instead of a system notification (0051) | Read and Delete as real buttons, Undo, a disabled Delete with its reason, the contact picture and two lines of snippet; desktop notification actions only show as an "Options" menu on macOS and cannot be updated or removed | `newMailStyle: system` per user; remove `NewMailStyle` and the popup window |
| Every address a contact, with a kind; pictures in the database, encoded in the helper; "mark as spam" is the stage 16 block (0050) | the user asked for every address, own pictures and spam marks; ranks stay as they were (only own and personal mail rank), one block list, pictures transactional and synced like the rest | `ContactFilter::People` hides lists and system addresses; pictures can be dropped without touching the rest |
| "Mark folder as read" as one outbox operation per folder on the stored unread UIDs (0063) | thousands of unread mails would be thousands of `set_flag` rows and (with CONDSTORE) one STORE per MODSEQ; `1:*` would also mark mail the user never saw; undo stays exact | per-message `markRead` on all unread locations (capped at 10,000 per action) |
| Gmail via IMAP with its extensions instead of the Gmail API (0034) | labels, one mail via `X-GM-MSGID` and archiving work with the existing adapter, without a Google project | Gmail API in block C |
| Windows installer via cross-build from the Mac | no Windows build machine needed | build on real Windows or hosted runners in stage 15 |
| Four crates instead of eleven | only draw boundaries once they have proven themselves | split as soon as a crate has two clearly separate tasks |
| Licence GPL-3.0-or-later (confirmed by the maintainer on 2026-11-22) | derivatives stay open; all chosen libraries are compatible | can be changed before publication |
| tauri-specta instead of ts-rs (0004) | typed commands and events, enforced decision for 64-bit numbers | ts-rs generates the same types; only `ipc.ts` would have to be rewritten |
| Stalwart as test server (0001) | one binary, all needed capabilities, no Docker | Dovecot via Homebrew with a separate MTA |
| FTS5 with `unicode61` instead of `trigram` (0003) | trigram is 5.2 times as large and half as fast at inserting | tokenizer can be switched via migration, the index is cache |
| A trigram index next to the word index, the mail text included (0074) | the human chose search inside words; numbers and German compounds stand mostly in the text; first hits stay far under 200 ms, the cost is disk (about 3.7 bytes per byte of text) | `SUBSTRING_BODY = false` and a rebuild: headers and file names only |
| `contact:` as a filter resolved on every search, phrases in `from`/`to`/`cc` of the word index (0074) | saved searches follow merged contacts; word boundaries keep addresses exact | an `OR` of `from:`/`to:`/`cc:` terms written into the query |
| Guided first start (0044): device setting `first_start`, `getFirstStart`/`finishFirstStart`; installations with an account count as set up | a new installation is guided through account, look and the optional extras, without existing installations seeing any of it | remove `FirstStartGate`, then the empty state again |
| Updates only with consent via the Tauri updater (0037) | the only connection to a server the user has not set up; signed files, key only in the keychain | switch off; without the updater you install by hand |
| rusqlite 0.41 instead of 0.43 (0006) | `rusqlite_migration` 2.7 requires ^0.41; same SQLite 3.54.1 | own migration runner, then rusqlite 0.43 |
