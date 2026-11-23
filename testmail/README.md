# testmail: local IMAP/SMTP test server

A local [Stalwart](https://github.com/stalwartlabs/stalwart) mail server (version 1.1.1,
one binary, IMAP and SMTP in one process) for development and integration tests. It only
listens on `127.0.0.1`, has two test accounts and can be filled with a deterministic set of
test messages. Real mailboxes are never involved.

Stalwart is used as a development tool only and is not shipped with Vice Mail
(license: AGPL-3.0 / Stalwart Enterprise License, dual-licensed).

## Commands

```
scripts/testmail.sh start            # start; downloads and provisions the server on first use
scripts/testmail.sh stop             # stop (SIGTERM, SIGKILL after 20 s)
scripts/testmail.sh status           # state, ports, memory, data size; exit code 3 when stopped
scripts/testmail.sh seed [N]         # fill alice's account with N messages (default 1000)
scripts/testmail.sh reset            # delete data and certificates, provision again (server stays stopped)
scripts/testmail.sh check            # prove TLS, IDLE, CONDSTORE, MOVE, SPECIAL-USE, keywords, SMTP rules
scripts/testmail.sh log [N]          # last N lines of the server log
```

A reproducible state for a test run: `scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000`.

The first `start` downloads `stalwart-<target>.tar.gz` from the GitHub release v1.1.1, checks its
SHA-256 against the value pinned in `scripts/testmail.sh` and keeps the binary in
`testmail/bin/` (about 110 MB). Supported: macOS (arm64, x86_64) and Linux (x86_64, aarch64).
Requirements: `bash`, `curl`, `openssl`, `python3` (standard library only).

## Ports (all on 127.0.0.1)

| Port  | Protocol | TLS | Authentication |
|-------|----------|-----|----------------|
| 1143  | IMAP | STARTTLS (LOGIN is refused before STARTTLS) | required |
| 1993  | IMAP | implicit TLS | required |
| 1587  | SMTP submission | STARTTLS | required (AUTH PLAIN, LOGIN after TLS) |
| 1465  | SMTP submission | implicit TLS | required |
| 1025  | SMTP | plain, STARTTLS offered | none; accepts only local recipients |
| 18080 | HTTP (JMAP, management API) | none | required |
| 18443 | HTTPS (JMAP for the client, stage 14) | implicit TLS | required |

The server names its JMAP endpoints with `https://localhost:18443` (`STALWART_PUBLIC_URL`), so
a client follows them to this port. A store provisioned before stage 14 gets the HTTPS listener
at the next `start`; its data stays.

Nothing leaves the machine: relaying to other domains is refused for everybody, the only
delivery route is local, spam filter, DNSBL, GeoIP and web UI are disabled, and the server
process runs with `HTTP(S)_PROXY` pointing at a closed local port so that any remaining
download attempt fails immediately.

## Accounts

`testmail/accounts.env` holds host, ports, addresses, passwords and the CA path as shell
variables (`set -a; . testmail/accounts.env; set +a`). Login names are the full addresses.

| Address | Password | Purpose |
|---------|----------|---------|
| `alice@vicemail.test` | `alice-test-pw` | main test account, receives the seed |
| `bob@vicemail.test` | `bob-test-pw` | second account (small fixed set of messages) |
| `admin@vicemail.test` | `admin-test-pw` | server administrator (management API on port 18080) |

These are test values for this local server only, not secrets.

## TLS

`reset` (or the first `start`) creates a test CA and a server certificate for `localhost`,
`127.0.0.1` and `::1` in `testmail/.data/tls/` (`ca.pem`, `server.pem`, ECDSA P-256, server
certificate valid 825 days). Clients must load `ca.pem` explicitly; nothing is added to the
system keychain. Without the CA the certificate is rejected.

## Folders

Every account has `INBOX`, `Sent`, `Drafts`, `Archive`, `Junk` and `Trash` with SPECIAL-USE
attributes (`\Sent`, `\Drafts`, `\Archive`, `\Junk`, `\Trash`). The seed adds `Projects`,
`Projects/2026`, `Projects/Überprüfung` (non-ASCII, modified UTF-7 on the wire) and the empty
`Projects/Empty`. The hierarchy delimiter is `/`.

## Seed

`seed N` stores N messages in alice's account (minimum 145, the fixed corpus) and 23 in bob's.
The result depends only on N and `--image-host`: fixed random seed, fixed dates and
Message-IDs. Every message has a header `X-Vicemail-Seed: <category>/<name>`, for example
`thread/offsite-07`, `charset/koi8-r`, `broken/truncated-base64`, `filler/00042`.

| Category | Content |
|----------|---------|
| `thread/` | one thread of 60 messages with branches, In-Reply-To/References (one without In-Reply-To, one without References), across Archive, INBOX and Sent |
| `charset/` | ISO-8859-1/-2/-15, Windows-1252/-1251, KOI8-R, ISO-2022-JP, Shift_JIS, EUC-JP, GB2312, Big5, EUC-KR, UTF-8 with emoji and RTL text, UTF-16, raw UTF-8 headers (RFC 6532), RFC 2047 in B and Q |
| `mime/` | alternative, mixed with PDF/PNG/CSV/ICS/ZIP, related with `cid:` image, inline images, forwarded `message/rfc822`, S/MIME signed, PGP/MIME encrypted, calendar invitation, digest, file names in RFC 2231 (with continuation) and RFC 2047, hostile file names (`../`, `CON`, U+202E, control characters), a draft, many keywords |
| `html/` | newsletters and a showcase with external images, CSS, fonts, `srcset`, tracking pixels pointing to `--image-host` (default `http://127.0.0.1:18999/`), hostile HTML (scripts, event handlers, `javascript:`, forms, deceptive links) |
| `list/`, `auto/` | List-Id, List-Unsubscribe (with One-Click), List-Post, Precedence, Auto-Submitted |
| `broken/` | 26 malformed messages: wrong or missing boundary, truncated and invalid base64, unknown or wrong charset, 8-bit header bytes, impossible or missing date, missing headers, broken encoded words, over-long lines, 40 nested multiparts, unknown transfer encoding, bare LF, broken addresses, duplicate Message-ID, reference loop |
| `large/` | a 2 MB and a 7 MB message |
| `smtp/` | 5 messages delivered to alice through SMTP (submission with STARTTLS, implicit TLS, and the unauthenticated local port) and 2 from alice to bob |
| `filler/` | the rest: plain, HTML, attachments, newsletters, notifications, short reply threads, spread over all folders from 2023 to 2026 |

Flags and keywords on the seed: `\Seen`, `\Flagged`, `\Answered`, `\Draft`, `$label1`–`$label5`,
`$Forwarded`, `$MDNSent`, `$Junk`, `$NotJunk`, `$Phishing`, `projekt-x`, `Steuer2026`.

Options: `--image-host URL`, `--filler-folder FOLDER` (all filler into one folder; with
`seed 10000 --filler-folder INBOX` the INBOX holds 9959 messages), `--no-smtp` (alice gets
N−5), `--force` (seed into a non-empty account), `--dump DIR` (only write `.eml` files),
`--index-timeout S`. `seed` refuses to run
when alice's INBOX is not empty; use `reset` first.

## Server behaviour worth knowing

- Custom keywords are returned lowercased (`Custom_Keyword-123` becomes `custom_keyword-123`,
  `Steuer2026` becomes `steuer2026`); registered ones such as `$Forwarded`, `$MDNSent`, `$Junk`
  keep their case. IMAP keywords are case-insensitive, so compare them that way.
- The server adds its own keywords `$hasattachment` / `$hasnoattachment` to every message.
- IMAP `SEARCH` goes through an asynchronous full-text index: a message is found a few
  milliseconds up to about a second after APPEND or delivery. `seed` waits until the index is
  complete. `SEARCH HEADER` matches words, not exact substrings.
- `ENVELOPE` is built from the parsed message: 8-bit header bytes appear as U+FFFD inside an
  RFC 2047 word, a missing From becomes `unknown@localhost`, an invalid date is normalised.
  Use `BODY.PEEK[HEADER]` when the raw header matters.
- SMTP delivery of a message whose Message-ID already exists in the recipient's mailbox is
  accepted with 250 but silently dropped (`message-ingest.duplicate` in the log). IMAP APPEND
  stores duplicates. Tests that send the same message twice must expect one copy.
- `UIDNEXT` and `MODSEQ` keep counting after `check` (it removes its messages again).
- Stalwart is an OAuth 2.0 authorization server too (stage 14, decision 0042): metadata at
  `/.well-known/oauth-authorization-server`, open dynamic client registration (`/auth/register`,
  redirect `http://127.0.0.1/` for native clients, any port), authorization code with PKCE S256
  only, token `/auth/token` (refresh tokens rotate), revocation `/auth/revoke` (public clients send
  `client_id`). The sign-in page `/login` uses the JSON API `/api/auth` (`authCode`, then
  `authConsent`), which `crates/mail-engine/tests/oauth_server.rs` drives as the "browser". IMAP
  offers `AUTH=XOAUTH2` and `AUTH=OAUTHBEARER`; `provision.py` adds both to SMTP submission (after
  TLS), so a store provisioned before 23 November 2026 needs `reset` for SMTP with a token.

## Files

| Path | Content |
|------|---------|
| `scripts/testmail.sh` | control script, pinned version and checksums |
| `testmail/accounts.env` | ports, accounts, CA path |
| `testmail/provision.py` | configures a fresh data store through the management API (JMAP) |
| `testmail/seed.py` | message generator and loader (IMAP APPEND with LITERAL+, SMTP) |
| `testmail/check.py` | capability and rule checks |
| `testmail/common.py` | small shared IMAP/TLS helpers |
| `testmail/.data/` | data store, certificates, logs, PID file (git-ignored) |
| `testmail/bin/` | the Stalwart binary (git-ignored) |
