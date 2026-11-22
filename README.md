# Vice Mail

An open-source email client for macOS and Windows, built with Tauri 2, a Rust core and a React
interface. Self-hostable in the sense that it needs nothing but your own mail servers: no cloud
service of ours, no account with us, no telemetry. "Vice Mail" is a working title.

## Status

**Pre-alpha.** The project is being built in stages (see `docs/PLAN.md`). There is no usable
release yet. Do not point it at a mailbox you care about.

Planned for version 1: IMAP/SMTP accounts, a fast unified inbox that stays smooth with hundreds of
thousands of messages, offline full-text search, safe HTML rendering with remote content blocked by
default, an outbox that never loses or duplicates mail, encrypted settings sync through your own
mailbox, and optional AI features through a provider you choose (local or with your own API key).

## Building

Requirements and exact setup steps are in `docs/SETUP.md` (macOS on Apple Silicon; the Windows
installer is cross-built from macOS). In short:

```sh
# Rust (stable, at least 1.88), Node.js 24, pnpm, Xcode command line tools
pnpm --dir app install
pnpm --dir app tauri dev        # run the app in development mode
scripts/check.sh                # format, lint, tests, type check, license and security checks
```

Integration tests run against a local test mail server:

```sh
scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000
```

## Documentation

Planning and design documents under `docs/` are written in German until the project is published.

| File | Content |
|---|---|
| `docs/PLAN.md` | stages and their status |
| `docs/ARCHITECTURE.md` | stack, module boundaries, mail engine rules, security boundaries |
| `docs/SPEC.md` | feature scope and what is out of scope |
| `docs/DESIGN.md` | interface design |
| `docs/decisions/` | recorded decisions with measurements |

## License

GPL-3.0-or-later. See `LICENSE`.
