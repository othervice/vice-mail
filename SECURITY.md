# Security policy

An email client sees passwords and every message, so security reports are taken seriously and
handled before anything else.

## Reporting a vulnerability

**Please do not open a public issue, pull request or discussion for a security problem.**

Report it privately in one of these ways:

1. **GitHub private vulnerability reporting:** on the repository page open **Security → Report a
   vulnerability** (<https://github.com/othervice/vice-mail/security/advisories/new>). This is the
   preferred way.
2. **Email:** `security@<domain>` <!-- TODO maintainer: replace with the real address before publishing -->

Please include:

- the version (Settings → About shows version and commit) and the operating system;
- what an attacker can achieve, and under which conditions (for example "a received message
  can …", "another local user can …", "a malicious MCP client can …");
- steps or a sample message (`.eml`) that reproduce it. Remove any real personal data from samples.

What happens next:

- You get an acknowledgement within 7 days. The project is maintained by one person; if you hear
  nothing after that, please send a short reminder.
- The problem is confirmed or explained, a fix is prepared in private, and a release with the fix is
  published. The advisory names you unless you prefer otherwise.
- Please give us up to 90 days from your report before you publish details, or less once a fixed
  release is out. If a problem is already being exploited, say so and it will be handled faster.

There is no bug bounty.

## Supported versions

Vice Mail is pre-alpha. Only the **latest release** receives security fixes; there are no
backports to older versions. Please update before reporting, and check whether `main` is
affected as well.

| Version | Supported |
|---|---|
| latest release | yes |
| older releases | no |
| `main` | yes (fixes land here first) |

## What is in scope

Everything in this repository, in particular:

- rendering of untrusted mail: HTML sanitising, the sandboxed frame and its Content Security
  Policy, link checks, remote image blocking, attachment previews, `.eml` files;
- the boundary between the web view and the Rust core (commands, events, capabilities, windows);
- handling of passwords and keys (keychain, the `Secret<T>` type, logs, error messages);
- the outbox and anything that could send, delete or move mail without the user's action;
- the settings sync encryption and its format ([docs/SYNC.md](docs/SYNC.md));
- the local MCP server and the AI features: access beyond what the user allowed, prompt injection
  that leads to actions;
- the in-app updater: anything that makes the app accept an update not signed with the project's
  update key (minisign key ID `CBF2BA7119C9327E`);
- network connections the app makes without the user's consent (the allowed list is in
  [docs/PRIVACY.md](docs/PRIVACY.md)).

What the app protects and what it does not is described in the threat model,
[docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md).

## What is out of scope

- Problems in mail servers, AI providers or AI clients themselves; report those to their makers.
  (How Vice Mail limits what an AI client receives *is* in scope.)
- Attacks that need administrator rights or full control of the user's account on the computer.
  The local database and stored mail are not encrypted by the app; disk encryption (FileVault,
  BitLocker) protects them at rest. This is a documented limit, not a vulnerability.
- The warnings of Gatekeeper and SmartScreen for unsigned builds; they are expected and explained
  in [docs/INSTALL.md](docs/INSTALL.md).
- Reports produced only by automated scanners without a demonstrated impact.
