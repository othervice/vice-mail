# Security model

This is the threat model of Vice Mail: what the app protects, against whom, where the trust
boundaries are, how each boundary is enforced, and what is explicitly **not** protected. It is
the result of the security review of stage 15 (23 November 2026) and lists every finding of that
review with its status. How to report a vulnerability is in [SECURITY.md](../SECURITY.md); the
network connections the app makes are listed in [docs/PRIVACY.md](PRIVACY.md) and, with the
technical rules, in `docs/ARCHITECTURE.md` ("Allowed network connections").

This review was done by the project itself. It does not replace an independent review by a third
party, which is advisable before wide distribution.

## 1. Assets

| Asset | Where it lives | Why it matters |
|---|---|---|
| Mail account passwords (IMAP/SMTP) | System keychain (macOS login keychain, Windows Credential Manager), service = bundle id, account = account UUID | Full control of the mailbox |
| OAuth refresh tokens of accounts that sign in with Microsoft or Google (stage 14, decision 0042) | System keychain (`oauth:<provider>:<account>`, split into pieces of at most 1,000 characters); access tokens only in memory | Mailbox access until revoked or expired, without the password |
| AI provider API keys | System keychain (`ai-profile:<id>`) | Billing, access to the provider account |
| MCP client keys | System keychain (app side); the AI client's own config file (client side, as `MAIL_MCP_TOKEN`), or a file only the user may read that the config names (`MAIL_MCP_TOKEN_FILE`) | Read access to the shared accounts, draft creation |
| Settings-sync key and passphrase | The random X25519 key: locally in the keychain (`settings-sync:key`), in the sync folder only wrapped with the passphrase (age scrypt); the passphrase is never stored | Decrypts every synced record; whoever holds it can also write records (server changes wait for the user on every device, F1) |
| Update signing key (minisign/Ed25519) | Only in the login keychain of the build Mac (`org.vicemail.updater`) | Whoever has it can ship code to every installation |
| Mail content, headers, attachments, contacts | Local SQLite database and zstd body files in the app data directory; the mail server | Privacy of the user and their correspondents |
| Integrity of the mailbox | Mail server; local outbox (`operation` table) | No mail lost, sent twice, sent, moved or deleted without the user |
| The user's attention and trust | The UI | Phishing, spoofed links, deceptive attachments |
| Reading behaviour (when/where mail is opened) | Not stored remotely | Tracking pixels, read receipts, link pings |

## 2. Attacker models

| # | Attacker | Capabilities assumed | Main goals |
|---|---|---|---|
| A1 | **Malicious sender** | Sends arbitrary MIME: HTML, CSS, attachments, headers, calendar invitations, `.eml` files the user opens, forged `From` | Run code in the WebView or the core, reach the Tauri IPC, track reading (remote content, link pings, DNS prefetch), phishing via spoofed links or names, crash/hang the app, smuggle instructions to an AI (prompt injection), trick the user into running an attachment |
| A2 | **Malicious or compromised mail server** | Full control of IMAP/SMTP responses after a valid TLS handshake for the configured host; can also host the settings-sync folder | Memory/CPU exhaustion through responses, corrupt local state, read the synced settings, make the client write where it must not |
| A3 | **Network attacker** | Sees and changes traffic between the app and any server (public Wi-Fi, hostile resolver); can answer DNS | Steal credentials, downgrade TLS / strip STARTTLS, redirect account discovery, inject updates, reach the user's local network through the app (SSRF) |
| A4 | **Malicious local app or other local user** | Runs code on the same machine. (a) *another OS user*; (b) *a process of the same user* | (a) read the mail, use the MCP channel; (b) see Section 6, out of scope |
| A5 | **Compromised AI or MCP client** | A registered MCP client (or the model behind it) acting on injected instructions; an AI provider returning hostile output | Read accounts not shared, send/delete/move mail, exfiltrate mail via links or images in AI output |
| A6 | **Malicious update host / manifest** | Controls the update manifest (`latest.json`) or a download host, or GitHub's file hosts | Install code that is not from the project, downgrade to a vulnerable version, make the app contact other servers |
| A7 | **Supply chain** | Publishes a malicious version of a dependency (crates.io, npm) | Code execution at build time or in the shipped app |

## 3. Trust boundaries

```
                         ┌───────────────────────────── untrusted ───────────────────────────────┐
                         │ mail content (A1)   mail servers (A2)   network (A3)   update host (A6) │
                         │ AI providers / MCP clients (A5)                                         │
                         └───────┬───────────────────┬──────────────────┬────────────────┬─────────┘
                                 │ MIME bytes        │ IMAP/SMTP        │ HTTPS          │ JSON-RPC
                                 │                   │ over rustls      │ (discovery,    │ over Unix socket
                                 │                   │ (B4)             │ images, AI,    │ (0600, peer UID)
                                 │                   │                  │ updater) (B4)  │ / named pipe + HMAC (B6)
┌────────────────────────────────▼───────────────────▼──────────────────▼────────────────▼─────────┐
│ Rust core (trusted): mail-engine, mail-imap, mail-content, mail-store, mail-ai, mail-mcp host       │
│  - B2 parse + sanitise (mail-content, pure, no IO, panics contained)                               │
│  - B5 secrets only via keychain, Secret<T> redacts                                                 │
│  - B7 every write through the outbox; read-only adapter for accounts without write access         │
│  - B8 updater: pinned endpoint/hosts, minisign signature bound to the version                     │
└───────┬───────────────────────────────────────────────────────────────────────────┬───────────────┘
        │ typed Tauri commands / events (B3), capability `default` for main,        │ keychain, files,
        │ message-*, composer-* windows only                                         │ OS "open" (B5, B9)
┌───────▼──────────────────────────────────────────────────────────┐        ┌──────▼──────────────┐
│ WebView main frame (trusted app code, CSP script-src 'self')      │        │ Operating system     │
│  ┌─────────────────────────────────────────────────────────────┐  │        │ (keychain, browser,  │
│  │ B1 mail frame: srcdoc iframe, sandbox="allow-same-origin",  │  │        │ default apps)        │
│  │ no scripts, meta CSP default-src 'none'; img-src data:,     │  │        └──────────────────────┘
│  │ links = https://link.invalid/<token>/<n> placeholders,      │  │
│  │ navigation hook in Rust (+ WebView2 frame guard on Windows) │  │
│  └─────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

| Boundary | Between | Enforced by |
|---|---|---|
| B1 | Mail HTML ↔ app UI | Sanitiser, mandatory meta CSP, `sandbox="allow-same-origin"` without `allow-scripts`, link placeholders, navigation hook, new-window refusal, WebView2 frame guard |
| B2 | Raw MIME / attachments ↔ core | `mail-content` (no IO, linear pre-check, limits), panics contained, decoders in Rust, previews and the images of mails re-encoded in the helper process (C-2, C-4) |
| B3 | WebView ↔ core | Typed commands only; no fs/shell/http/dialog/updater plugin permissions for the UI; paths come from core-owned dialogs, never from arguments |
| B4 | Core ↔ network | rustls (aws-lc-rs) with the platform verifier, typed certificate errors, private-network block for content fetched on behalf of mail, host allowlists |
| B5 | Core ↔ OS secrets | Keychain only, `Secret<T>`, no secrets in DB, logs, errors or diagnostics |
| B6 | MCP client ↔ core | Local socket/pipe only, per-client HMAC challenge-response, per-account permission, tools cannot send/delete/move |
| B7 | User intent ↔ mailbox | Outbox with visible state, read-only mode, explicit decisions for unclear sends, no automatic read receipts |
| B8 | Update host ↔ installed app | Consent, endpoint and host allowlist, minisign signature with `requireSignedVersion`, no downgrades |
| B9 | Attachments / links ↔ OS | Sanitised file names, save only where the user chose, links only `http`/`https`/`mailto` after a confirmation that shows the real target |

## 4. What is protected, and how

### 4.1 Mail rendering (A1 → B1, B2)

- **No script execution.** HTML mail is shown in a `srcdoc` iframe with `sandbox="allow-same-origin"`
  and nothing else; with `allow-scripts` added, a script reached `parent.__TAURI_INTERNALS__` in
  spike D (decision 0005), so it is never added. Tauri injects its scripts only into the main frame.
  The sanitiser (ammonia allowlist) removes scripts, event attributes, forms, frames, objects,
  `base`, `meta refresh`, `link`, SVG and MathML anyway (defence in depth).
- **No network access from mail content.** External URLs in `src`, `srcset`, `background`, CSS
  `url()`/`@import`/`image-set` are replaced by placeholders until the user allows remote content;
  the CSS is rewritten token by token (no regex). In the dark theme (stage 21, decision 0066) a
  colour pass runs on the already sanitised CSS and attributes and only replaces colour values
  with colours it writes itself, removes `color-scheme` declarations and decides
  `prefers-color-scheme` queries; it adds no property, selector, attribute or URL, and the hostile
  corpus and the fuzz test render every mail in both schemes. Every document starts with the meta CSP
  `default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action
  'none'` and `referrer no-referrer`. Images are only `data:` URIs the core wrote: inline
  images, allowed remote images and `data:` URLs in the HTML (PNG, JPEG, GIF or WebP by their
  bytes; never SVG, BMP, AVIF or ICO) are decoded in the `mail-mcp` helper process and encoded
  again (PNG/JPEG, animations as GIF), so the WebView's image decoders never see a mail's bytes;
  what the helper refuses is not shown (C-4). Allowed remote images are fetched by the core, not
  by the WebView.
- **Links never navigate.** Every `href` becomes `https://link.invalid/<render token>/<index>`;
  only the core knows the target. The navigation hook in Rust intercepts the click (the frame may
  only load `about:srcdoc`/`about:blank`; everything else is cancelled), the UI shows the real
  target and warnings (mismatch with the text, punycode/mixed scripts, look-alikes, user info, IP
  hosts), and only after a click on "Open" does the system browser get it, for `http` and `https`
  only; `mailto:` opens the app's own composer. The UI passes only a render token and an index,
  never a URL. `target`, `ping`, `download` and `<link>` (preconnect) are removed. New windows
  and web-view downloads are always refused. The sanitised output is parsed again and checked
  against the allowlist (every `href` a placeholder of its own token with an index in range,
  every `src` a `data:image/`); if the check fails, the mail is shown as text.
- **Linear pre-check** against inputs that would make html5ever quadratic (more than 150,000
  tags, depth over 1,500, too many open formatting elements); such mail is shown as text. The
  pre-check reads the HTML like html5ever's tokenizer (comments, attributes, raw text, SVG and
  MathML content, CDATA) and refuses what it cannot model; a differential fuzz test compares it
  with the parser (finding C-1).
- **Contact pictures** (stage 17, decision 0050) take the same way: the file the user chose
  (system dialog or a drop the shell saw, at most 20 MiB, read by the core) is decoded and
  encoded again in the helper (`mail-mcp --contact-picture`, same limits and watchdog), first for
  the cropper, then as the stored 256 px JPEG; a picture from the settings sync is encoded again by
  this device's helper before it is shown. Never a picture from the network.
- **Attachments are never rendered by the WebView.** Previews are decoded in Rust (images re-encoded,
  PDF rasterised by hayro without scripts or network, text cleaned), within pixel, page and byte
  limits; HTML, XHTML, XML and SVG are never previewed. A decoder panic becomes `preview_damaged`.
  Images and PDF are decoded in a child process (`mail-mcp --render-preview`), which the core
  kills after 20 s or above 512 MiB of resident memory (C-2): a decoder that runs away ends the
  child, not the app.
- **File names** are sanitised (path separators, `..`, control and bidi characters, reserved
  Windows names, length) and files are written only where the user chose in the system dialog,
  never overwriting an existing file.
- **Subjects and display names** lose bidirectional controls (U+202A–202E, U+2066–2069,
  U+200E, U+200F, U+061C) wherever headers are cleaned: IMAP envelopes (`mail-imap`), parsed
  mails, contacts, invitations and rule matching (`mail-content`), with one predicate
  (`mail_model::is_bidi_control`) shared with the file-name sanitiser (C-8).
- **Hidden text and invisible characters** are removed from what goes to an AI (MCP or built-in):
  the model gets what the reader sees, including only the summary of a closed `<details>` (C-6).

### 4.2 The WebView ↔ core boundary (B3)

- The UI only talks to the core through typed commands and events (`app/src/services/ipc.ts` is
  the only importer). Capabilities grant only event listen/unlisten, window dragging and, for
  secondary windows, closing themselves. No fs, shell, http, dialog, notification or updater
  permission.
- Windows are opened only by the shell, with the same navigation hook, new-window refusal,
  download refusal and WebView2 frame guard for every window; which window calls a command is
  taken from the calling `WebviewWindow`, never from an argument. The app's own commands are not
  restricted per window by capabilities (no `AppManifest` command list): every window the shell
  creates is equally trusted, and no other web content is ever loaded into one (finding V-09).
- The app never opens a web page itself except, after a click on "Sign in with Microsoft/Google"
  or "Sign in again", the provider's fixed sign-in address in the **system browser**, through the
  same opener as links (decision 0042). The answer comes back to a loopback listener in the core,
  not to the WebView.
- No command takes a file path from the UI. Files are read only from paths the OS hands over
  (`.eml` open events and arguments; drag and drop onto the window, accepted only when the OS
  reported the drop, within 120 seconds and once) or the user picks in a core-owned dialog.
  Draft files go through an allowlist of the draft's own directory.
- App CSP: `script-src 'self'`, no remote sources, `object-src 'none'`, `base-uri 'none'`,
  `frame-ancestors 'none'`. `style-src 'unsafe-inline'` and `frame-src 'self' http: https:` are
  needed for mail CSS and for the navigation hook to see link clicks (decision 0005); they do not
  allow scripts.

### 4.3 Network (A3 → B4)

- TLS everywhere through rustls with the `aws-lc-rs` provider and the operating system's
  certificate verification (`rustls-platform-verifier`); TLS 1.2 and 1.3 only; no OpenSSL.
  The certificate must match the configured host. Certificate errors are typed (untrusted, wrong
  name, expired) and shown; there is no "accept anyway". Extra roots only in tests and the
  desktop-test build.
- Mail servers: there is no plaintext mode (security is TLS or STARTTLS). STARTTLS must be
  offered and accepted, otherwise the connection fails (no fallback, so no stripping); bytes
  received before the TLS handshake are discarded; `PREAUTH` is refused; credentials are sent
  only inside TLS. Every connection has connect, read and write deadlines.
- Account discovery contacts DNS (SRV) and the domain's own autoconfig over HTTPS only after the
  user clicks "search online", and shows what it found before anything is saved; a found server
  outside the address's domain or given as an IP address gets its own warning (N-4). No
  third-party directory (no ISPDB).
- Content fetched on behalf of a mail (allowed remote images, one-click unsubscribe) never goes to
  private, loopback, link-local, CGNAT, multicast or reserved addresses; names are checked after
  resolution and the connection uses only the checked addresses. No cookies, referrer, proxy or
  retries; neutral user agent; strict size and time limits.
- AI providers: only the configured base URL, `http` only to local addresses, no redirects,
  proxies or retries; cloud providers only after consent and only with mail from accounts the
  user released for the cloud; a stored key only to the server it was entered for (V-02).
- Sign-in with Microsoft and Google (decision 0042): OAuth 2.0 authorization code with PKCE (S256)
  and a 256-bit `state`, in the system browser, redirected to `127.0.0.1`/`::1` on a random port.
  The listener accepts exactly one `GET /` carrying the `state` (constant-time comparison, no
  repeated parameters, 16 KiB and 5 s per request); forged answers, other paths and other ports
  change nothing, the wait ends after five minutes or on cancel. Token endpoints are fixed per
  provider, HTTPS only, no redirects, proxy, cookies or retries, 64 KiB answers. Access tokens are
  only handed to the provider's own IMAP/SMTP hosts (a list in the profile), so a changed server
  setting (or a settings-sync record) cannot send a token elsewhere; they go out only with
  `XOAUTH2`/`OAUTHBEARER`, never as a LOGIN/PLAIN password.
- No telemetry, no crash reports, no other connections.

### 4.4 Secrets (B5)

- Passwords and keys only in the system keychain (`<account uuid>`, `mcp-client:<id>`,
  `ai-profile:<id>`, `settings-sync:key`); the database holds only references (`credential_ref`,
  `has_key`). Secrets are wrapped in `Secret<T>` right at the IPC boundary; it formats as
  `[redacted]` in `Debug`/`Display` and has no `Serialize`; keychain error texts are stripped of
  stored data; tests prove that secrets do not appear in logs or errors. The UI never gets a
  stored secret back (an MCP client token is shown once, at creation). Removing an account
  deletes its keychain entry first and stops if the keychain refuses; revoking an MCP client,
  deleting an AI profile and switching sync off delete their entries. An AI profile saved with
  another server (scheme, host or port) and no new key loses its key first, and nothing is saved
  if the keychain refuses (V-02).
- OAuth (decision 0042): the refresh token is a keychain entry like a password; `Credentials::get`
  refuses such entries, so no code path can send one as a password. Access tokens live only in
  memory. Neither appears in logs, errors, the database, the settings sync or the diagnostics export
  (tested). Removing the account revokes the grant where the provider offers revocation (Google)
  and deletes all pieces of the entry. Client IDs, and Google's installed-app "client secret"
  (which Google documents as not confidential), are public build configuration, not secrets.
- Logs are local files (14 days, our crates at `info` in release builds) without secrets,
  subjects, bodies or addresses logged on purpose; failed commands are logged with their code
  only (V-07, fixed). Error texts logged inside the core can still contain a path or an address.
  The diagnostics export (1fa77b5, after this review) includes the end of the newest three logs
  and redacts every line again; its test proves that no secret and no mail content of a seeded
  account reaches the file (`docs/PRIVACY.md`, "Diagnostics").
- Settings sync: records are encrypted with age to a random X25519 key (kept locally in the
  keychain as `settings-sync:key`); that key is wrapped with the passphrase (age scrypt, work
  factor 2^18 when writing, at most 2^20 accepted when reading, so a hostile server cannot make
  unwrapping arbitrarily slow). The mail server sees only opaque ids, versions, sizes and times.
  Record kind and content are inside the ciphertext and bound to the header through an HMAC-derived
  opaque id; entries listed above 2 MiB are never downloaded, a pass reads at most 5,000 entries
  and 64 MiB (F2), and ciphertexts over 1 MiB are refused before decryption; records from clocks
  more than five minutes ahead are deferred. `writes_enabled` is never synced, and accounts adopted from
  another device start read-only and without a password, under a keychain entry name of their
  own (never one this device already holds). A record that names other servers for an existing
  account does not change them: this device keeps its servers and password until the user
  confirms (password removed first, writing off) or dismisses the change (F1, decision 0040).
  The passphrase is never stored.

### 4.5 AI and MCP (A5 → B6)

- The MCP server accepts only local connections (Unix socket 0600 in a 0700 directory with a
  peer-UID check; Windows named pipe with a random name, first-instance and remote clients
  rejected); never TCP. Every connection proves possession of the client key by HMAC-SHA-256
  challenge-response in both directions, compared in constant time.
- Per account: invisible (default), read, or read and create drafts, checked again on every call.
  No tool sends, deletes, moves, flags or changes settings. Tools return sanitised text (what the
  reader sees), wrapped in an untrusted-data block with a random boundary, within size and count
  limits and a rate limit (120 calls per minute); bodies are fetched with `BODY.PEEK`. Drafts on
  accounts without write access stay on the device. Switching MCP off or revoking a client closes
  its connections at once. Trash and Junk are left out of searches unless named; reading by id
  follows the same rule: mail that lies only there needs that mailbox named, and a thread leaves
  it out (F4, decision 0045).
- The built-in AI has no tools at all. Its output is shown as text; links in it go through the
  same token/check/confirm path as links in mail, images are never loaded.

### 4.6 Write safety (B7)

- New accounts are read-only (`writes_enabled = false`); the adapter rejects every writing command
  on them with a typed error, and even with writes enabled, sync, IDLE and reading use read-only
  connections (`EXAMINE`, `BODY.PEEK`). The read-only allowlist checks every command line,
  including LOGIN/AUTHENTICATE/IDLE: `SELECT`, `STORE`, `APPEND`, `EXPUNGE`, `COPY`, `MOVE`,
  `CREATE`, `DELETE`, `RENAME`, `SUBSCRIBE` and a `FETCH` of `BODY[`/`BINARY[`/`RFC822` (which
  would set `\Seen`) are refused; `UID` only with `FETCH` and `SEARCH`. Writing happens only
  through the outbox connection, behind the `writes_enabled` check; SMTP refuses read-only
  accounts. The settings-sync connection may write into its one folder only (exact line check).
- Every write is an outbox operation with a visible state; an unclear send is never retried
  without the user's decision; moving between accounts never produces two copies or none.
- Read receipts are sent only on the user's explicit command; remote images load only after the
  user allowed them.

### 4.7 Updates (A6 → B8)

- Checks only with consent (off by default, asked once), at most daily, or on "Check now".
- Only the endpoint compiled into the app and GitHub's file hosts, HTTPS only, at most five
  redirects within those hosts, on `github.com` only the endpoint's own `/<owner>/<repo>/releases/`
  paths; a download URL outside them is refused before connecting. No app version or identifier
  is sent. Release notes are shown as text.
- The download must carry a minisign signature from the project's update key for exactly the
  announced version (`requireSignedVersion`); downgrades are never offered. Installation only
  after a click.
- Time and size limits: the manifest request has 30 s, the download 15 minutes in all (connect
  to last byte); the download stops as soon as more than 300 MiB are announced or have arrived
  (N-3). The manifest's size is not capped (it is read inside the updater plugin).

### 4.8 Supply chain (A7)

- `Cargo.lock` and `pnpm-lock.yaml` are committed and builds use `--locked` / frozen lockfiles;
  npm packages are pinned to exact versions; dependency build scripts of npm packages are off
  unless allowlisted (`app/pnpm-workspace.yaml`).
- `cargo deny` (advisories, licences, bans incl. OpenSSL/native-tls, crates.io as the only source)
  and `pnpm audit` over all npm dependencies, the desktop-test tree included (G-1), run in
  `scripts/check.sh`. Fixes in that tree come in as exact pnpm `overrides`; an advisory without a
  fixed version is ignored only by its GHSA id, with the reason (`app/pnpm-workspace.yaml`).
- `unsafe` is forbidden in all own crates except `webview2-guard`, checked by `scripts/check.sh`.
- The test mail server binary is downloaded with a pinned SHA-256.

## 5. What is explicitly NOT protected

- **Data at rest.** The SQLite database, body files, logs and attachment copies in the app data
  directory are not encrypted by the app. Disk encryption (FileVault, BitLocker) protects them
  when the machine is off or locked. Decision in `docs/ARCHITECTURE.md` ("No database
  encryption in version 1").
- **A malicious process running as the same user** (A4b). It can read the database and body files,
  read the MCP client token from the AI client's config file or token file, attach to or inject
  into the app's processes, and on macOS may get keychain access with the user's consent prompt.
  Desktop operating systems do not isolate apps of the same user from each other (outside the App
  Store sandbox, which this app does not use).
- **What a cloud AI or MCP client does with mail it was given.** The app limits what it hands out
  and marks it as untrusted; prompt injection against the client's own tools is outside the app.
- **Sender authenticity.** `From` can be forged. The app does not evaluate SPF/DKIM/DMARC results
  in v1; "always load images from this sender" therefore trusts a forgeable address (impact:
  tracking pixels load).
- **Discovery results** (DNS SRV, autoconfig) are not DNSSEC-validated. They are shown to the user
  before use; a server outside the address's domain or given as an IP address carries its own
  warning (N-4), and certificate verification still applies to the chosen host, so a forged answer
  naming a host inside the domain fails at the TLS handshake. Whether the user heeds the warning
  for a host elsewhere (hosted domains often point to their hoster) is up to the user.
- **Opening an attachment with the system.** After the user confirms (and always after an
  explicit confirmation for types that can run code), the operating system's default application
  handles the file; what that application does is outside the app. Every file the app writes
  from an attachment carries the system's "downloaded" mark (Windows mark of the web, macOS
  quarantine attribute), so SmartScreen, Office Protected View and Gatekeeper apply as for a
  browser download.
- **Server-side metadata.** The mail server sees every mail, flag and the size and timing of the
  sync records. The settings sync hides content, not activity.
- **Denial of service by the user's own mail server** beyond the limits listed in the findings.
- **Unsigned first releases** (decision 0035): until Developer ID signing and notarisation are
  active, users must trust the download channel (HTTPS from GitHub, SHA-256 sums in the release).

## 6. Residual risks

- **The HTML guard is a model of the tokenizer.** The linear pre-check (`guard.rs`) keeps html5ever
  away from inputs that would make it quadratic, by reading the HTML the way the tokenizer does.
  The review found three places where it read differently (finding C-1, fixed); where its model
  of SVG and MathML cannot follow the parser it now refuses, and a differential fuzz test
  compares it with the real parser. Another, unknown difference would
  reopen the same denial of service: a mail that hangs or exhausts memory while its body is
  stored, indexed or shown, without a click. A structural limit inside the parse (or parsing in a
  separate process with a time and memory limit) would remove the dependency on the model;
  this is the recommended next step.
- **Decoders without their own resource limits** (hayro for PDF, `image` for pictures, also for
  the images of mails, C-4) run in a child process with a 20 s and 512 MiB limit (C-2). The child
  runs with the user's rights and no sandbox; within its limits it can still use a CPU core for
  20 s. The memory watchdog looks every
  50 ms, so a fast-growing child can pass the budget by what it allocates in that time. Without the
  helper (a development build) the decoders run in the app.
- **Server-side denial of service.** A malicious mail server (or one that hosts the sync folder)
  can make the app allocate up to 512 MiB per IMAP literal (N-1), hold a command open by trickling
  bytes (I-4) or send unbounded folder lists. The settings sync reads no entry listed above 2 MiB
  and at most 5,000 entries and 64 MiB per pass (F2, fixed); a server that lists a smaller size
  than it sends is still bounded only by N-1, and entries it adds can fill a pass and delay the
  rest (they are reported, never deleted). The user chose that server; the effect is memory
  pressure or a stuck account, not code execution or data loss.
- **Settings sync trusts every holder of the sync key** for everything except credentials: a
  holder (one compromised device, or the provider of the sync account after cracking a weak
  passphrase) can change names, identities, signatures, rules and other synced settings on every
  device. Server changes wait for the user, and the stored password never goes to servers named
  only by the folder (F1, fixed). A new device takes over accounts with the folder's servers; the
  user sees them on the account's page before entering the password there.
- **Image decoders in Rust instead of the system's.** The images of mails (and the images the
  composer shows) are decoded by the `image` crate in the helper process and encoded again
  (C-4); the WebView's decoders (WebKit/ImageIO, WebView2) see only PNG, JPEG and GIF the helper
  wrote. A decoder bug in `image` is contained by Rust and the helper's limits (C-2); a helper
  taken over through one could still hand the WebView crafted bytes in these three formats,
  which needs such an exploit first. Without the helper (a development build) the images are
  decoded in the app.
- **Phishing.** Link warnings, the display of the real target and the executable-type
  confirmation reduce, but cannot remove, the risk that a user follows a convincing message.
  Bidirectional-text controls are removed from subjects and sender names (C-8); look-alike
  characters in names are not.
- **Prompt injection.** Hidden text is removed before mail goes to an AI, but the rules cannot
  cover every way to hide text (the module `visible` lists what it does not recognise: text over
  images or other elements, transparent strokes, blurred shadows, image masks, among others), and
  visible text can carry instructions too. The built-in
  AI has no tools; an MCP client has its own tools, outside the app's control.
- **The update channel** depends on the secrecy of one signing key on one machine, and on GitHub
  serving the manifest. A compromised release can make the app download at most 300 MiB (or for
  15 minutes) before the signature check rejects it (N-3); a manipulated manifest is read whole
  within its 30 s limit.
- **Windows-specific code is reviewed but not tested on Windows** in this review (WebView2 frame
  guard, named pipe, mark of the web, installer).
- **OAuth sign-in (decision 0042).** During a sign-in any local program can connect to the loopback
  port, but without the `state` it can neither finish nor break it; it cannot see the provider's
  page in the browser. Whoever holds a stolen refresh token reads the mailbox until it expires or is
  revoked; at Microsoft the app cannot revoke it (no revocation for public clients). The client IDs
  are public, so another app could present itself with them; PKCE keeps it from using codes meant
  for this app, but users see this app's name on its consent screen. Tested against a local OAuth
  server and a mock, not against Microsoft or Google; not on Windows (keychain size limit, `::1`).

## 7. Findings of the stage-15 review

The review covered, in this order: (a) the WebView boundary, (b) the IPC surface, (c) the
HTML/MIME pipeline, (d) network, (e) secrets, (f) write safety, (g) supply chain. It was done by
reading the code and the sources of the dependencies involved (`~/.cargo/registry`), with tests
written for every fix. Severity: **high** = reachable by a received mail or a network attacker
without user action, with significant impact; **medium** = significant impact that needs a user
action, a key the attacker should not have, or a malicious server; **low** = limited impact or
unlikely preconditions; **info** = hardening or documentation.

### 7.1 Fixed in this review

| ID | Area | Severity | Finding | Fix and test |
|---|---|---|---|---|
| C-1 | c | high | The HTML guard read the input differently from html5ever in three places: comments that end early (`<!-->`, `<!--->`, `--!>`), attribute names that start with `=` (`<a ="…`), and `style`/`title`/`script`/`textarea` inside SVG and MathML (markup there, raw text for the guard). Deep nesting or a formatting bomb behind such a prefix reached the parser: zero-click CPU/memory exhaustion while a body is stored, indexed or shown (the parser is quadratic in depth; the formatting bomb can abort the process). Confirmed with a test before the fix. | `guard.rs`: comments end as in the tokenizer, `=` may start an attribute name, a model of foreign content (SVG/MathML entry, namespaces — `<svg>` inside MathML stays MathML, found by the campaign —, HTML and MathML text integration points, breakout tags including `font` with `color`/`face`/`size`, end tags) decides about raw text and CDATA sections; where the model cannot tell what the parser does (an end tag inside SVG for an element that may or may not still be open outside, or SVG nesting beyond the depth limit) the guard refuses, and the mail is shown as text. Tests: `guard::tests::tokenizer_differences_do_not_hide_nesting` (27 prefixes, an overflow case, 3 controls that must stay accepted), differential fuzz test `the_guard_sees_the_nesting_the_parser_builds` (guard accepts ⇒ html5ever's tree is not deeper than the limit), run in the campaign below. Residual risk: Section 6. |
| V-01 / C-3 | b | medium | macOS: files written from attachments (open, save, save all, `.eml` attachments) had no quarantine attribute, so Gatekeeper never checked them; an `.app` unpacked from an attached `.zip` (no confirmation, not an executable type) started without the Gatekeeper prompt. | `reading/files.rs`: `com.apple.quarantine` (`0081;<time>;Vice Mail;`) via `/usr/bin/xattr` with fixed arguments, set before the copy for opening is made read-only (neither system lets the owner mark a read-only file; this also fixes the Windows mark of the web on copies for opening, not verified on Windows). Test: `reading::files::tests::written_attachments_are_quarantined_on_macos`. The Gatekeeper prompt itself was not checked in a GUI session. |
| V-03 / C-10 | b, c | low | Types that run code or are current phishing vectors opened without the executable-type confirmation: `.rdp`, `.one`, `.iqy`, `.slk`, `.appref-ms`, `.vsto`, `.ppkg`, `.wsb`, `.theme`, `.diagcab`, `.searchconnector-ms`, Access databases, `.pyz`, `.msu`, `.website`, certificates, macOS `.shortcut` and others. | `filename.rs`: 31 extensions and 6 MIME types added. Test: `filename::tests` (table extended). |
| V-06 | a | low | No download handler: the web view's own downloads (context menu "Download Linked File", "Save Image As") were not refused. | `windows.rs`: every window refuses downloads (`on_download` returns `false`, logged with scheme and host only). Test: `windows::tests::the_web_view_never_downloads`. |
| C-7 | c | low | The "link text names another domain" warning was suppressed by invisible characters or look-alike full stops in the link text (`paypal\u{200B}.com`, `paypal．com`). | `links.rs`: the text is stripped of hidden characters and look-alike dots are read as dots before the comparison. Test: `links::tests::text_naming_another_domain_warns`. |
| N-2 | d | low | The private-network block for remote images and one-click unsubscribe let through IPv4-translated (`::ffff:0:a.b.c.d`), local-use NAT64 (`64:ff9b:1::/48`), Teredo and the rest of `2001::/23`, discard (`100::/64`) and the new documentation range (`3fff::/20`). | `reading/remote.rs`: refused (the embedded IPv4 address decides for IPv4-translated). Test: `reading::remote::tests::only_public_addresses_are_contacted` (10 new cases, 4 public controls). |
| V-08 (part) | b | low | `remove_draft_dir`/`remove_outgoing_dir` joined the raw draft id to the compose directory; a draft id from the UI is not validated when deserialised (`"../.."`), so only the store's earlier refusal of unknown ids kept `remove_dir_all` inside. | `compose/files.rs`: both go through `resolve`, the allowlist every other compose path uses. Test: `compose::files::tests::draft_directories_are_removed_only_inside_the_compose_directory` (removes `../../victim` with the old code). |
| V-07 | b | low | The IPC bridge logged every command error text; some contain file paths (drag and drop), addresses or contact search text, which then sat in the local logs (and would go into a diagnostics export). | 1fc5a77: the bridge logs command, code and the fixed summary of the error kind (`logging::command_failed`); only `internal` and `database` errors add their detail, redacted (home paths as `~/…`, other paths as `<path>`, addresses, hosts, secret-looking values). The diagnostics export (1fa77b5) redacts every log line again in the same way, with the accounts' names and folder names. Tests: `tests::failed_commands_are_logged_without_personal_error_texts`, `diagnostics::tests::the_file_holds_facts_and_counts_but_no_secret_and_no_mail_content` (error texts with a home path and an address). Error texts themselves are unchanged and still reach the UI's technical details. |
| F1 | e | medium | A synced account record that changed IMAP/SMTP host, port, security or user name was applied as is (also when a local account was matched by address on joining); the stored password then went to the new server and `writes_enabled` stayed as it was. A record for a *new* account with the UUID that a re-keyed local account keeps as its keychain entry name would even have used that password. Whoever has the sync key got the passwords of the other accounts on every device. | Decision 0040: server settings are a credential boundary. A pass keeps this device's servers and records the folder's in `account_server_change`; the account shows "Server settings changed on another device". Confirming removes the password from the keychain first (nothing changes if that fails), switches writing off, then writes the servers and asks for the password; dismissing keeps the servers and is remembered. Accounts taken over from the folder get a random keychain entry name. Tests: `settings_sync::tests::server_change` (through `AppCore` with a connector that records host and password of every login and an in-memory sync folder; an attacker with the sync key writes into it: changed servers, dismissing, a new account under a planted keychain entry, an account matched by address on joining — no login ever reaches the attacker's hosts; all four fail on the old code), `settings_sync_tests::other_servers_from_the_folder_wait_for_the_user`, `adopted_accounts_get_a_fresh_keychain_reference`, `app/tests/server-change.test.tsx`. |
| C-4 (part) | c | medium (defence in depth) | The image sniffer for mail content also accepted BMP, AVIF and ICO, which then went to the WebView's system decoders as `data:` URIs (inline, allowed remote, and `data:` URLs in the HTML). | `image.rs`: only PNG, JPEG, GIF and WebP are recognised; the others are shown like any image that cannot be shown (the element stays without a source). The independent checker of the fuzz harness accepts only these four. Tests: `image::tests::sniffing_recognises_rasters_and_rejects_the_rest`, `html::tests::bmp_avif_and_ico_images_are_not_shown`. Re-encoding the remaining four: C-4 below. |
| C-2 | c | medium | hayro decompressed PDF streams without a size cap and the preview had no time limit (blocking pool, not cancellable): 2,253 bytes of PDF expanded to 1 GiB and reached 1.1 GB in 0.37 s; running out of memory ends the process. | Decision 0043: image and PDF previews run in `mail-mcp --render-preview` (`mail_mcp::preview`): file on stdin, one JSON reply on stdout; the core kills the child after 20 s or above 512 MiB resident memory (`/bin/ps -o rss=` / `tasklist`, every 50 ms) and maps that to `preview_too_large`, a crash to `preview_damaged`; a release build without the helper refuses these previews. No new dependency, no `unsafe`. Tests: `mail-mcp/tests/preview_isolation.rs` against the real helper: the 1 GiB sample stops as `preview_too_large` in 0.35 s (release; 1.9 s debug) with the parent's memory unchanged, a normal PDF renders (92–102 ms release), the time limit, a failing and a missing helper; `preview::tests` (parsing of `ps` and `tasklist` output). Windows (`tasklist`, `mail-mcp.exe`) untested. |
| C-9 | c | low | An image preview at its limits could use about 1.3 GB (64 megapixels decoded, a copy to turn it upright, conversion for the encoder; more at 16 bits per channel). | `attachment_preview.rs`: before decoding, the decoded size from the header (pixels × bytes per pixel of the colour type) counted twice must stay within 384 MiB (`MAX_PREVIEW_DECODE_BYTES`), so 64-megapixel RGBA and large 16-bit images are refused as `preview_too_large`; an 8-bit RGB photo of 64 megapixels still fits. The preview also runs in the helper with its 512 MiB budget (C-2). Test: `attachment_preview::tests::large_images_are_scaled_and_huge_ones_refused` (8000×8000 RGBA and 6000×6000 16-bit refused from the header, 7000×7000 RGB passes the check). |
| G-1 | g | low | `pnpm audit` in `check.sh` covered production dependencies only; the dev tree had 10 advisories (4 high), all in the WebdriverIO desktop-test chain (`serialize-javascript`, `extract-zip`, `basic-ftp`, `glob`, `minimatch`). | `app/pnpm-workspace.yaml`: exact `overrides` with the vulnerable range as selector: `serialize-javascript` 6.0.2 → 7.1.4 (mocha), `basic-ftp` 5.3.1 → 6.2.6 (get-uri), `glob` 10.5.0 → 14.0.6 (`@wdio/config`, archiver-utils), which also replaces `minimatch` 9.0.9 with 11.0.9; the consumers only use API the new majors keep (`serialize()`, `Client.access/lastMod/list/downloadTo/close`, `glob.sync()`). `extract-zip` 2.0.1 (`@puppeteer/browsers`) has no fixed version: its two advisories (GHSA-jmr9-qjv8-65gv, GHSA-7pqw-9j4j-h8q3) are ignored by id (`auditConfig.ignoreGhsas`, with the reason); it only unpacks browser downloads, which `@wdio/utils` makes for Chrome and Firefox, never for the desktop tests (`browserName: "tauri"`, WebDriver inside the app). `scripts/check.sh` now runs `pnpm audit` over all dependencies. Tests: `pnpm audit` (only the 2 ignored) and `pnpm audit --prod` clean; before and after, the same specs from `wdio.conf.mjs` through `@wdio/config` (22 files in order, plus a `*.e2e.mjs` pattern) and the same `archiver-utils` `file.expand` result; mocha in parallel mode (the path that uses `serialize-javascript`); get-uri loads basic-ftp 6 with the methods it calls; `wdio --version`; Vitest, typecheck, ESLint, `scripts/licenses.mjs --check`. The desktop tests themselves were not run (GUI session). |
| F4 | e | low | MCP `read_message`/`read_thread` returned mail that lay only in Trash or Junk when its id was known (ids are sequential), although the searches leave both out unless the call names them. | Decision 0045: both tools take an optional `mailbox_id`; a message whose every location is Trash or Junk counts as missing unless the call names that mailbox (`mail_mcp::readable_in`, applied in the core's `CoreAccess`, `mcp/access.rs`), with the same text as an unknown id or a hidden account's message; `read_thread` leaves such members out before it picks the newest 25, and from `total`; `create_draft` refuses `in_reply_to` such a message (its quote would reach the Drafts folder). Mail that also lies elsewhere stays readable. Tests: `mcp::tests::reading_by_id_leaves_out_mail_only_in_trash_or_junk` (mail-engine, real store: Trash, Junk, a message in Inbox and Trash, thread window and total; fails on the old code), `contract::mail_only_in_trash_or_junk_is_read_only_when_its_mailbox_is_named` (mail-mcp, through rmcp, helper and host: identical refusals for Trash, Junk, a wrong mailbox, a hidden account and a missing id; threads; the reply draft), `access::tests::mail_only_in_trash_or_junk_needs_its_mailbox_named`. |
| F2 | e | low | Sync-folder entries were fetched whole, without a size or count limit; the 1 MiB ciphertext limit applied only after download and dearmoring, so a server that hosts the folder could make every device allocate large amounts on each pass. | `settings_sync/transport.rs` (`FetchLimits`), `plan.rs`, `pass.rs`: what a pass downloads is decided from the listed size (`RFC822.SIZE`, `RemoteEntry.size`) before anything is read. An entry listed above 2 MiB is never downloaded, never counts as a version and is never deleted (not as a loser, damaged or misplaced entry, nor as an old copy of the key record; joining does not read such key records); it is reported (`Notice::TooLarge`). A pass downloads at most 5,000 entries and 64 MiB: records in the order of their opaque ids, each with all of its new entries or not at all; a record that does not fit waits as a whole (`Notice::Postponed`), later ones that fit still come, and a full check re-checks known entries only with the room left. `service.rs` counts both among the status's ignored entries (text in `en.ts`/`de.ts` adjusted), runs the next check even with an unchanged folder, and a pass that postponed a record does not count as complete. Tests: `settings_sync::tests::limits` (oversized entries under a record's id with a newer version, under an unknown id and as a newer key-record copy: never read, never deleted, the rest syncs, also through full checks and a later join; a cap of 4 entries works through 11 entries in 3 passes without deleting a postponed entry, then both devices converge with changes on both sides; a byte cap; a record larger than a whole pass waits while the others sync; a full check under a cap downloads the new entry first and holds nothing back — all fail without the limits), `transport::tests::limits_belong_to_the_clone`, `server_change::an_entry_listed_too_large_is_not_read_kept_and_counted_in_the_status` (through `AppCore` and the IMAP transport: a 3 MiB message is never read, stays, and the status counts it). A server that lists a smaller size than it sends is still bounded only by N-1. |
| C-6 | c | low | Hidden-text detection missed the content of a closed `<details>`, transparent text whose `text-shadow` is transparent too (any shadow counted as drawing the text), text made tiny by `zoom`, and masks that let nothing through. Hidden instructions could reach an AI (MCP or built-in). | `visible/mod.rs`: in a closed `<details>` only the first `<summary>` child is read (also through elements the sanitiser unwraps; text directly in the element and later summaries are left out). `text-shadow` is read per layer: a layer draws the text only where its colour (`currentcolor` where it names none, resolved where the text is) passes the alpha and contrast check of the text colour and it is not moved 500 px off the left or top edge; invalid values are ignored as by the browser. `zoom` is multiplied through the ancestors into the font size (below 4 px hidden); `zoom: 0` and `0%` count as 1, because WKWebView and Chromium (Brave 157, headless) were measured to draw them at full size, so they hide nothing. `mask`/`mask-image` (also `-webkit-`) whose layers are all `none` or gradients with only transparent colour stops hide the element and its content (`currentcolor` included); image, partly transparent and luminance masks count as showing. `text-shadow` and mask values over 2 KiB count as showing nothing (bounds the cost per element). `visible/cascade.rs` reads the new properties, and presentational attributes (`open`, `hidden`, …) count only with a value the sanitiser keeps (`html.rs`: `presentational_value`, shared with the sanitiser). Each rule and its controls were compared with screenshots in WKWebView and headless Chromium. Tests: `visible::tests::closed_details_show_only_their_first_summary`, `text_shadows_draw_text_unless_they_do_not_show_either`, `zoom_scales_the_text`, `masks_that_let_nothing_through` (each with positive controls: open `<details>`, a shadow that shows, `zoom:1` and `zoom:0`, masks that show), `hidden_text_corpus` (`hidden-text-display.eml`, `hidden-text-colors.eml`: 7 new hidden and 5 new visible markers), `mail-mcp/tests/hostile.rs` (`injection-hidden-html.eml`: a closed `<details>` and a transparent shadow through `read_message`); the corpus and MCP tests fail on the old code. Not recognised (module documentation of `visible`): strokes in a transparent colour, shadows blurred beyond reading, image, partly transparent or luminance masks. |
| F5 | e | low | The suggested Claude Code setup command carried the MCP token as an argument (`--env MAIL_MCP_TOKEN=…`): visible in `ps` while it ran and kept in the shell history. | `helper.rs`: the helper also reads the token from `MAIL_MCP_TOKEN_FILE` (absolute path of a file that holds only the token, at most 4 KiB); on Unix a file that group or others may access (`mode & 0o077`) is refused, with the mode and `chmod 600` in the error on stderr and in every tool result; the mode is checked on the opened file. `MAIL_MCP_TOKEN` keeps working; if both are set, the file wins (warning on stderr), and an unusable file is an error, never a fall back. `config.rs`: Claude Code also gets the same command with `--env "MAIL_MCP_TOKEN_FILE=$HOME/.mail-mcp-token"` (`McpClientConfig.tokenFileContent`, never the token); the settings show it with its own copy button, explain how to create the file, and say that the inline command ends up in the shell history (avoidable with a leading space where the shell supports it). "Test connection" removes `MAIL_MCP_TOKEN_FILE` from the helper's environment. Windows: the file's access list is not checked, the `$HOME\…` variant is untested. Tests: `helper::tests` (file, both variables, six group/other modes refused also when `MAIL_MCP_TOKEN` would work, 0600 and 0400 accepted, missing, malformed, empty, oversized, a folder; no error text contains the token), `binary::the_token_can_come_from_a_private_file` and `a_token_file_others_may_read_is_refused` (the shipped helper: works with the file over the real socket; refuses 0644 on stderr and in the tool result without connecting), `config::tests::claude_code_also_gets_a_command_with_a_token_file`, `mcp::tests::clients_live_in_the_keychain_and_revoking_removes_them`, `app/tests/mcp-settings.test.tsx` (hint, key-file command without the key, copy). |
| V-02 | b | low | Changing an AI profile's base URL kept the stored API key (consent was reset, but "test connection" and "list models" sent the key), so a typo or a misleading instruction sent the key to another host. | `ai/settings.rs`: saving an existing profile with a key whose base URL now has another origin (scheme, host, port; `url::Url::origin`, so the case of the host and an explicit default port do not count) and "keep the key" (`AiKeyChange::Keep`) removes the key instead (`same_server`); `ai/mod.rs` `save_ai_profile` deletes the keychain entry first and saves nothing if that fails (the keychain error reaches the UI). The profile then has no key and reports `keyRemovedOnServerChange` until it is saved again: the settings show "Key removed: the server changed" at the provider, the form asks for the key again, and before saving the form already says that the key will be removed (`AiSettings.tsx`, en/de). A change of the model or the path keeps the key; a new key in the same save is stored for the new server. The model list and the connection test only use the saved profile, and every reader takes profile and key together under the settings lock, so a save that changes server and key is entirely before or after it. Tests: `ai::tests::server_change::another_server_removes_the_kept_key_before_anything_is_sent` (host, port and scheme changes: key gone from `MemorySecretStore`, the providers the model list and the connection test build for the new server get no key; fails on the old code), `a_model_or_path_change_keeps_the_key` (model, path, `https://AI.Example.Test:443`), `a_failing_keychain_delete_refuses_the_save_and_changes_nothing` (injected delete failure: `keychain` error, settings and key unchanged, the key still goes only to its server; fails on the old code), `cloud_providers_need_consent_and_see_only_permitted_accounts` (adjusted), `app/tests/ai-settings.test.tsx` (hint before saving, notice after, the key asked for again, none for a path change). |
| N-4 | d | low | Servers found online (DNS SRV, the domain's autoconfig, a JMAP server it lists) were not flagged when they lay outside the address's domain or were IP literals; the user saw them but got no specific warning (RFC 6186 §6). | `mail-engine/src/discovery/warning.rs`: every server of an online result is checked against the address's domain (ASCII/IDNA form, case-insensitive, trailing dot ignored; the domain and its subdomains are inside): an IPv4/IPv6 literal (also the IPv4 forms the resolver reads, such as `0x7f.1`) is `ipAddress`, any other host outside the domain is `outsideDomain`; the exact hosts of the built-in provider table are exempt (Workspace/Microsoft 365 domains point there; a password sent there reaches that provider), and results of the built-in table never carry a warning. Discovery asks no MX records, so no provider domain found by MX is trusted. `DiscoveryResult.warnings` (`ServerWarnings`, at most one per IMAP, SMTP and JMAP server). The setup form (`AddAccount.tsx`, `ServerFields.tsx`, `ProtocolChoice.tsx`) shows a warning per flagged server in the summary and in the server form, as long as the host is still the found one, before anything is saved; it warns, the user decides. Tests: `discovery::warning::tests` (subdomain, other domain, look-alike `example.org.evil.test`, IPv4, IPv6, trailing dot, case, IDN, hosts that are no names, table hosts), `discovery::tests::srv_targets_outside_the_domain_or_as_ip_addresses_are_flagged`, `autoconfig_servers_are_flagged_per_server`, `idn_domains_compare_in_their_ascii_form`, `known_providers_carry_no_warning`, `warnings_on_the_wire`; Vitest `account-setup.test.tsx` ("warnings for servers found online": summary, form, JMAP, host changed and typed again, other domain, German). |
| C-4 | c | medium (defence in depth) | `cid:` images, allowed remote images and `data:` URLs in mail HTML were sniffed and passed unchanged as `data:` URIs to the WebView, whose system decoders (ImageIO/WebKit, WebView2) parsed them: a decoder vulnerability in the OS was reachable by a received mail without a click. | Decision 0046. `render_html` shows an image only as its re-encoding (`RenderOptions::images`, keyed by the original bytes) and lists the others (`RenderedHtml::pending_images`); the core re-encodes them in one batch per rendering in the helper (`mail-mcp --reencode-images`, `preview.rs`: `run_reencode_child`, `reencode_isolated`, same 20 s / 512 MiB watchdog as C-2) and renders again (`reading/mod.rs`, `reading/images.rs`). `reencode.rs`: decoded within the preview limits (64 MP, 192 MiB decoded, C-9), at most 2048 px, PNG/JPEG out; animated GIF and WebP as animated GIF (at most 300 frames, 12 Mi pixels, 16 MiB, else the first frame); a batch on up to eight cores within one 384 MiB budget. Only `data:image/(png|jpeg|gif);base64` from the helper reaches a document (`is_reencoded_data_url`); refused images and failed batches are shown like any image that cannot be shown, never as the original bytes. Results are kept in memory (keyed SipHash, 64 MiB), refusals too. A release build without the helper refuses the images; tests and development builds without it re-encode in the process with the same code. The composer's images (draft inline images, also of drafts of other programs; signature images) go the same way (`Reading::editor_images`). Cost: the 32-image newsletter takes 18–21 ms through the helper in a release build (the first helper start of a session 77–100 ms once), 0.4 s in a debug build (`docs/testing/performance.md`). Tests: `reencode::tests::*` (new bytes with the same size, animated GIF stays animated, animated WebP becomes GIF, frame and edge limits, refusals, batch order, data-URL form), `html::tests::images_are_shown_only_as_their_re_encoding`, `allowed_and_inline_images_become_data_uris`, `reading::images::tests::*` (cache bounds, one batch then cache hits), `reading::tests::mail_images_are_shown_re_encoded_and_kept` (batch count unchanged on the second view), `without_a_working_helper_mail_images_are_not_shown` (missing helper, `/usr/bin/false`), `editor_images_are_re_encoded_too`, `mail-mcp/tests/reencode_isolation.rs` (real helper: re-encoded, animated, damaged, too large, unsupported; missing, failing and stopped helper; ignored release measurement), fuzz: `mutated_images_are_refused_or_reencoded` also through `reencode_image`, the checker accepts only PNG/JPEG/GIF and `foreign_images` finds any image the re-encoder did not write (`hostile_corpus.rs`, `fuzz.rs`, `seed_messages.rs`). Windows and the desktop tests: not run. |
| N-3 (rest) | d | low | The update download had no size or overall time limit (the update a check returns has `timeout: None`; the plugin buffers the whole body). | `updater.rs`: the download gets the plugin's request timeout (15 minutes, connect to last byte) and the same limit around the whole download; the progress callback marks a download over 300 MiB (announced by `Content-Length` or counted), and the download future is dropped at its next poll, which closes the connection. Tests: `updater::tests::a_download_over_the_size_cap_stops` (64 MiB offered with and without `Content-Length`, cap lowered to 256 KiB: fails as `update_download_failed`, less than 16 MiB leave the server), `a_download_over_the_time_limit_stops` (limit lowered to 0.4 s, a ten-second download stops within 4 s). The manifest's size: still open (7.2). |
| C-8 | c | low | Bidi controls (U+202A–202E, U+2066–2069; also the marks U+200E, U+200F, U+061C) stayed in subjects and display names (list, reading header, contacts), so `Invoice\u{202E}fdp.exe` showed as `Invoiceexe.pdf`. | `mail_model::is_bidi_control` / `strip_bidi_controls`, used by `mail-imap` `convert::decode_text` (envelope subjects and names, BODYSTRUCTURE names) and in `mail-content` by `parse::clean_text` (subjects, names of whole mails and `.eml` files), `contacts::clean_display_name`, `calendar::lines::clean_line` (invitation titles and names) and the rule-matching header cleaner; `filename.rs` uses the same predicate. Right-to-left text itself stays. Tests: `convert::tests::subjects_and_names_lose_bidi_controls` (raw, encoded words, a FETCH envelope), `parse::tests::subjects_and_names_lose_bidi_controls`, `contacts::name::tests::display_names_are_cleaned`, `calendar::lines` tests, `text::tests::every_bidi_control_goes_and_nothing_else`. Headers stored before this change keep them until the local copy is rebuilt (new UIDVALIDITY, account added again); no version had been published. |
| N-3 (part) | d | low | On `github.com` an update manifest could point to any repository's release files (the signature check still applied). | `updates/http.rs`: on `github.com` only paths under the endpoint's `/<owner>/<repo>/releases/`. Test: `updates::http::tests::github_endpoint_allows_its_file_hosts_over_https_only`. Size and time limits of the download: N-3 (rest). |

### 7.2 Open, with reason

| ID | Area | Severity | Finding | Impact | Proposed fix | Why not now |
|---|---|---|---|---|---|---|
| N-1 | d | low | async-imap allocates the announced literal size (up to 512 MiB) before data arrives; mail-imap adds no cap. | A malicious server can make each connection hold up to 512 MiB until the 30 s timeout. | Check `RFC822.SIZE` before `BODY.PEEK[]`, fetch large bodies in parts, get a configurable maximum into async-imap. | Needs a library change. |
| N-3 (manifest) | d | info | The update manifest is read whole (`res.json()` inside the updater plugin), with the 30 s request limit but no size cap. | A manipulated manifest (GitHub or the release compromised) can make the app hold a large JSON document once per check. | Fetch the manifest in our own code with a size cap and hand the plugin only the result, or a cap in the plugin. | Not in our code: the plugin offers no hook for the response; checks run only with consent, at most daily, over the pinned endpoint. |
| V-04 | a | low | App CSP `frame-src 'self' http: https:` is broader than needed (only `https://link.invalid` placeholders must reach the hook). | If a navigation ever passed the sanitiser while the WebView2 guard is inactive, the frame could load a web page. | `frame-src 'self' https://link.invalid`, checked with the desktop link tests on WebKit and WebView2. | Needs the desktop tests on both platforms (decision 0005 measured the current value). |
| V-05 | a | low | The WebView2 frame guard fails open: an installation error is only logged, and a window whose `finish` fails stays open. | Unchecked frame navigations on Windows (today only placeholders exist in the frame). | Close the window or render mail as text for it. | Windows-only, not testable here. |
| F3 | e | low | The Windows named pipe keeps the default access list. | Other local users can open instances (they cannot authenticate) and use up the 16 connection slots. | Owner-only security descriptor (current user and SYSTEM). | Setting security attributes needs an `unsafe` call, which the project allows only in `webview2-guard`; Windows-only. |
| C-5 | c | low | CPU cost grows with parts × HTML size (`cid:` search per image part, linear lookups). | Slow rendering of crafted mail with thousands of parts (measured here: 100,000 parts parse in 0.3 s debug; the render path was not measured). | One pass over the HTML for referenced `cid:`s, maps instead of linear search. | Performance work. |

### 7.3 Information and hardening notes

- **I-1** Autoconfig sends the full address as `?emailaddress=` to `autoconfig.<domain>` (after
  the click, `discovery/autoconfig.rs`). `docs/PRIVACY.md` said only the domain is sent; the
  text now matches the code (the query is unchanged, as Thunderbird sends it too).
- **I-2** DNS SRV lookups use the resolvers from the system configuration directly (plain UDP),
  bypassing encrypted DNS configured in the OS.
- **I-3** LOGIN is used for ASCII credentials even if the server announces `LOGINDISABLED` (only
  after TLS; leads to a misleading error).
- **I-4** No overall time limit per IMAP command (only per read), no cap on the number of folders.
- **I-6** The WebView runtime may connect on its own (WebView2 updates and SmartScreen, WebKit
  safe-browsing); not listed in the privacy text yet.
- **I-7** AI profiles over `http` to LAN addresses send key and mail unencrypted across the LAN
  (documented; a UI hint would help).
- **F6** "Export diagnostics" (stage 15 task) was being written while this review ran and is not
  covered by it. The logs it includes contain hosts of opened links and images, attachment
  temp paths and error texts with paths or addresses (V-07). Done in 1fa77b5 and 1fc5a77: hosts
  outside the provider table, paths, addresses and secret-looking values are replaced in every
  log line, with a test that none of a seeded account's secrets or mail content reaches the file;
  free text such as a subject cannot be recognised and must stay out of the logs.
- **F7** The sync passphrase minimum is 8 characters; changing it re-wraps the same key (after a
  leak, "reset" is needed, not "change").
- **F8** age's X25519 recipients do not authenticate the sender: records are accepted because the
  HMAC-derived opaque id binds kind and id, and the public key is never published. Both must stay so.
- **F9 / C-11** "Always load images from this sender" trusts the forgeable `From` (Section 5); the
  mail frame has no height cap.
- **F10** An MCP reply draft is created before its account is checked (unreachable today; the
  host checks first).
- **F11** `Secret<T>` does not zero memory on drop; a render token falls back to a predictable
  value if the OS has no randomness (logged; a mail cannot forge placeholders either way); the MCP
  socket falls back to the temp directory if the data directory is unusable.
- **V-08** Some ID types (`DraftId`, `RenderToken`, `EmlFileId`, signature and template ids)
  deserialise any string; they are checked by the store or case by case, not when the UI sends
  them. Validating in the type (`serde(try_from)`) is the clean fix (it touches the generated
  bindings). The one place where an unchecked id reached the file system is fixed (7.1).
- **V-09** App commands are not restricted per window by capabilities (see 4.2).
- **V-10** Script in the main window could do everything the UI can (add an account, enable
  writes, copy mail out, enable MCP). Prevented by `script-src 'self'`, no HTML sinks in the UI,
  and the sandboxed mail frame; optional hardening: native confirmations from Rust for these.
- **V-11** The macOS navigation hook also accepts `http(s)://tauri.localhost` (only meaningful on
  Windows); no way for mail content to produce it was found.
- **V-12** Windows: `.eml` arguments may be UNC paths (`\\host\share`); only reachable by argument
  injection, which the quoted `"%1"` registration prevents.
- **V-13** `set_notification_texts` has no length limit; `check-release.mjs` (no WebDriver in the
  shipped binary) is not part of the build scripts.
- **Supply chain checked and OK:** `Cargo.lock` and `pnpm-lock.yaml` committed, `--locked`
  everywhere, npm packages pinned exactly, npm build scripts off except an allowlist, `cargo deny`
  clean (advisories, licences, bans, sources: crates.io only), `pnpm audit --prod` clean, the
  Stalwart test server pinned by SHA-256, no git dependencies, `unsafe` only in `webview2-guard`.

## 8. Fuzzing

Fuzzing covers the MIME parser binding, the HTML pipeline, text extraction for AI/MCP, the
composer's HTML cleaners and the attachment-preview decoders. The harness is
`crates/mail-content/tests/fuzz.rs`: seeded mutation and generation with oracles specific to
this code, part of every `cargo test` run (a few seconds) and usable as a time-boxed campaign.

**Targets and oracles**

| Test | Input | Oracle |
|---|---|---|
| `mutated_mails_never_panic_and_stay_clean` | corpus mails (`testmail/corpus/seed`, `hostile`) with bit flips, MIME tokens, cuts, duplications | no parser panic; sanitised HTML passes the independent checker (re-parsed with `markup5ever_rcdom`, own allowlist, CSP first, links only own placeholders, every image one the re-encoder wrote, C-4); file names harmless; preview length; visible text for AI/MCP within its limit; calendar and read-receipt readers and text previews of attachments do not panic |
| `random_tag_soup_is_sanitised` | random tags, attributes and values from a hostile vocabulary | as above; the sanitiser's self-check never fails (the guard may refuse SVG/MathML soup it cannot model); `html_to_text`, `html_to_visible_text`; links open only `http`/`https`/`mailto` |
| `random_css_is_sanitised` | random style sheets and `style` attributes | checker finds no `url()`, `@import`, expressions …; visible-text rules do not panic |
| `the_guard_sees_the_nesting_the_parser_builds` | tokenizer-confusing prefixes (comments, quotes, SVG/MathML, raw text, CDATA, integration points) before 1,700–2,300 levels of nesting | differential: whenever the guard lets HTML through, html5ever's tree is not deeper than the guard's limit (finding C-1) |
| `composer_html_cleaners_never_let_active_content_through` | tag soup through `mail_html` and `clean_pasted_html`, also as `mailto:` | no active elements, event attributes, foreign URLs; images only `cid:` (none when pasted) |
| `mutated_images_are_refused_or_reencoded` | PNG (RGBA, grey, 16-bit), JPEG (with EXIF rotation), animated GIF, WebP, mutated with format tokens and boundary integers, PNG CRCs repaired | result is a re-encoded PNG/JPEG within 2048 px, or a typed error; as an image of a mail (`reencode_image`, C-4) also an animated GIF within the frame limit; decoder panics are counted (they are contained) |
| `mutated_pdfs_are_refused_or_rasterised`, `hostile_pdf_structures_are_contained` | PDFs with text, fonts, images, shadings, a self-drawing form, 5,000 nested `q`, a page tree containing itself, mutated with PDF tokens | PNG within the page limit or a typed error; no stack overflow, no escaped panic |
| `random_bytes_as_text_previews_stay_clean_and_bounded` | random bytes with BOMs, surrogates, escape sequences | text only, no control characters, within 512 KiB |
| `structure_bombs_end_within_the_limits` | multipart nested 20,000 deep, `message/rfc822` nested 20,000 deep, 100,000 parts | parse ends on a 2 MiB stack (the size of Tokio's blocking threads) |
| `random_file_names_become_harmless_and_stable`, `random_links_are_checked_without_panic` | random names and URLs | harmless and idempotent names; only allowed schemes open |

**Campaign controls:** `MAIL_CONTENT_FUZZ_SEED=<n>` (another reproducible input stream),
`MAIL_CONTENT_FUZZ_SECONDS=<n>` (each test runs whole rounds until the time is up),
`MAIL_CONTENT_FUZZ_ROUNDS=<n>`. Campaigns run optimised (`--profile fuzz`, a profile given on the
command line that inherits `release` without LTO), for example:

```
MAIL_CONTENT_FUZZ_SEED=20261123 MAIL_CONTENT_FUZZ_SECONDS=1200 cargo test -p mail-content \
  --test fuzz --locked --config 'profile.fuzz.inherits="release"' \
  --config 'profile.fuzz.lto=false' --profile fuzz -- --nocapture
```

**Why not `cargo fuzz`:** libFuzzer through `cargo fuzz` needs a nightly toolchain (sanitizer
and coverage flags); this machine has only stable Rust, and an instrumented build of the whole
dependency tree would have cost more of the short time box than it returns. The code under test
is safe Rust (`forbid(unsafe_code)` in our crates and in `hayro`) apart from one block in
`hayro-syntax` (a `transmute` for its self-referencing page list, decision 0043), so
AddressSanitizer would add
little; the bugs to expect are panics, hangs, memory blow-ups and policy violations, which the
oracles above check directly. The cost is that the generators are not coverage-guided: deep
states are reached by vocabulary and seeds, not systematically. Coverage-guided targets for
`parse`, `render_html`, the CSS sanitiser, `render_preview`, the calendar reader and
`parse_mailto` (nightly, in CI) are the recommended next step.

**Campaign of 23 November 2026** (Mac mini M4, optimised build, seed `20261123`, 16:45–17:11,
26 minutes wall time, up to 11 threads; the machine was shared with other builds, so the counts
are lower bounds of what an idle machine does):

| Test | Time | Cases | Result |
|---|---|---|---|
| mutated images | 20 min | 164,255,040 (20,763,123 re-encoded) | no failure, no contained decoder panic, slowest 0.31 s |
| mutated PDFs | 20 min | 3,877,986 (1,468,889 rendered) | no failure, no contained decoder panic, slowest 0.43 s |
| hostile PDF structures | once | 7 | contained (no stack overflow) |
| random CSS | 20 min | 21,698,957 | no violation |
| composer HTML | 20 min | 17,098,235 | no violation |
| guard vs parser | 17 min after the fix | first run stopped at once with a **failure at case 188** — `<math>…<svg><foreignObject><title>` hid 4,296 levels of nesting (`svg` inside MathML is MathML, so `title` was not raw text); fixed in `guard.rs` (C-1), regression test added; second run: 229,198 documents, 79,503 parsed, none deeper than the limit | fixed |
| random tag soup | 17 min after the fix | first run stopped at once, case 252: the test still demanded that the guard never refuses; with the stricter guard it may refuse SVG/MathML soup (shown as text), the test now only forbids a failed self-check; second run: 8,516,619 documents, no violation | test adjusted |
| mutated mails | until round 690, then 5 min | first run stopped at round 690: the independent checker flagged `javascript:` as text in a `role` attribute (harmless: `role` is a token list like `aria-*`, never a URL); checker corrected; second run (replays round 690): 4,339,080 mails, 951,665 HTML documents sanitised, no violation | checker adjusted |
| text previews | 5 min | 182,312,126 | no violation |
| file names | 5 min | 157,085,539 | all harmless and idempotent |
| links | 5 min | (not counted in that run) | no disallowed scheme opened |
| structure bombs | once | 3 | 32 ms (20,000 nested multiparts), 0.1 ms (20,000 nested `message/rfc822`), 23 ms (100,000 parts), on a 2 MiB stack |

The PDF decompression bomb of finding C-2 was found by code reading, not by the campaign (the
mutation vocabulary cannot produce valid compressed streams); it was confirmed with a sample
replayed through `replay_preview_file`. The temporary build directory `target/fuzz` was deleted
after the campaign.
