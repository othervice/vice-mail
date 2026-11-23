# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing has been released yet. This section describes what exists in `main` on the way to the first
public release (0.1.0). Internal test builds were made for the maintainer only and were never
published. How each part was built and tested is recorded per stage in `docs/stages/` (German).

### Added

**Accounts and sync**
- IMAP and SMTP accounts with password, TLS or STARTTLS; certificates are checked against the
  system's trust store, TLS through rustls.
- Server settings found from the address on request: DNS SRV records and the domain's own
  autoconfiguration, no third-party directory.
- New accounts are read-only: the app downloads and shows mail but changes nothing on the server
  until the user allows changes for that account.
- Gmail over IMAP with an app password: one message across labels, labels shown and changed,
  archiving as Gmail does it. Known settings and hints for iCloud, Yahoo, GMX, WEB.DE, Fastmail,
  Posteo and others (see `docs/PROVIDERS.md`).
- Fast synchronisation with IDLE, CONDSTORE and QRESYNC where the server offers them; message
  bodies stored compressed on disk and prefetched for offline reading.
- Passwords in the macOS keychain or the Windows Credential Manager only.

**Inbox**
- Unified inbox across accounts, single accounts and account groups; accounts can be hidden.
- A virtualised list that stays smooth with 500,000 messages.
- Conversations, on by default and in every inbox view.
- Categories without AI (people, notifications, newsletters) with a priority block, bundled rows
  and per-sender corrections.
- Pins, snooze, tasks, a reading list, favourite folders, folder colours, "unread only" and
  "unread first".
- Local rules (conditions on sender, recipient, subject, list or account; actions such as move,
  mark read, category, pin), running while the app runs.
- Blocking senders; moving and copying messages between accounts.

**Reading**
- HTML mail sanitised in Rust and shown in a sandboxed frame without scripts and with a strict
  Content Security Policy; plain text as fallback.
- Remote images and tracking pixels blocked until allowed once or always for a sender; allowed
  images are fetched by the core without cookies or referrer, and never from private network
  addresses.
- Every link shows its real target before it opens in the system browser, with a warning when
  text and target differ or the domain looks deceptive.
- Attachments: save, save all, open with the system after a check; previews of images, PDF pages
  and text rendered by the core; pinned attachments.
- Calendar invitations shown readably and handed to the system calendar.
- One-click unsubscribe (RFC 8058) on request.
- Print, save as PDF, export as `.eml`, open `.eml` files; messages in their own window; find in
  message.
- Read receipts only by the standard (RFC 8098): requested per message, never answered without
  asking.

**Actions**
- Read and unread, flag, archive, delete, move, spam, keywords, multiple selection and undo.
- Every change goes through an outbox that survives crashes and restarts, never applies an action
  twice, and shows its state; conflicts and server errors are shown, not hidden.

**Writing**
- Rich text and plain text, sent as `multipart/alternative` with mail-safe HTML; pasted HTML from
  Word or web pages is cleaned.
- Several identities, signatures per identity, templates (optionally with recipients), inline
  images, attachments by picker or drag and drop.
- Drafts saved locally and in the server's Drafts folder.
- Undo send, send later (while the app runs), "send and archive", inline quick reply, composer in
  its own window.
- Contacts collected from sent and received mail, ranked by use; names editable, duplicates merged.

**Search**
- Offline full-text search over all stored mail, first results while typing.
- Operators: `from:` `to:` `cc:` `subject:` `in:` `account:` `filename:` `has:attachment`
  `is:unread` `is:flagged` `is:snoozed` `before:` `after:` `older_than:` `newer_than:`, phrases and
  `-` to exclude.
- Saved and recent searches; continuing a search on the server.

**Settings sync**
- Accounts (without passwords), identities, signatures, templates, rules, saved searches, settings
  and more merged between devices through a hidden folder in a mailbox of the user's choice.
- Encrypted with age; the key is protected by a passphrase. The mail server sees only the number,
  size and timing of entries. Format in `docs/SYNC.md`.

**AI (optional, off by default)**
- Local MCP server: AI clients such as Claude Desktop, Claude Code, Cursor or LM Studio can search
  and read the accounts the user shares, and create drafts where allowed. No tool can send, delete,
  move or mark mail. Local connections only (Unix socket or named pipe), every client can be
  revoked, every access is listed.
- AI in the app with a provider the user chooses: any OpenAI-compatible server (Ollama, LM Studio,
  llama.cpp, OpenRouter, OpenAI) or the Anthropic API. Thread summaries, reply suggestions,
  writing help, questions about mail answered from local search results with sources, hints about
  unanswered mail.
- Before anything is sent to a cloud provider the app asks and shows the size of the transfer;
  cloud use is allowed per account. Mail content is treated as data, never as instructions; the
  built-in model has no tools.

**Interface**
- Three layouts (reading pane right, below, or list only), compact and comfortable density, an
  accounts and folders column.
- Full keyboard control with changeable shortcuts and conflict detection, a command palette
  (Cmd+K / Ctrl+K).
- Screen-reader roles and labels, visible focus, contrast checked in light and dark mode, text size
  80 to 150 %, respects "reduce motion".
- Light and dark mode following the system, six accent colours (orange by default).
- Native notifications per account with quiet hours, notification sounds, an unread badge on the
  dock or taskbar icon.
- Default mail app for `mailto:` links.
- English and German.

**Installation and updates**
- macOS disk image for Apple silicon and a Windows installer (x64), with SHA-256 checksums.
- In-app updates from the GitHub releases, verified against the project's update key and the
  announced version. Checking only when the user turns it on (asked once, off by default) or
  clicks "Check now"; downloading and installing only on click.
- Database backups before every migration; a database newer than the app is refused with a clear
  message.

### Known gaps

- The Windows build has not been tested on a Windows PC yet.
- The Windows installer is not code-signed. macOS signing and notarisation are prepared and start
  once the Apple developer membership is active; until then macOS builds are only ad-hoc signed.
- Outlook.com and Microsoft 365 (sign-in with Microsoft), "Sign in with Google" and JMAP are not
  supported yet.
- Spell checking uses the system's and has not been checked on either platform.
- Questions about mail in natural language are built and tested with a stand-in model only, not
  yet with a real one.
- Not planned for version 1: PGP and S/MIME, calendar and contact sync, POP3, encryption of the
  local database (use FileVault or BitLocker).

[Unreleased]: https://github.com/othervice/vice-mail/commits/main
