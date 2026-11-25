<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/wordmark-dark.svg">
    <img src="docs/brand/wordmark-light.svg" alt="Vice Mail" width="320">
  </picture>
</p>

# Vice Mail

A fast, private email client for macOS and Windows. Rust core, React interface,
[Tauri 2](https://tauri.app). It talks only to your own mail servers: no cloud service, no
account with us, no telemetry.

> **Pre-alpha.** New accounts start **read-only** until you allow changes per account.

![Vice Mail: unified inbox with an unfolded conversation and the reading pane](docs/images/inbox-light.png)

## Features

- **Accounts:** IMAP/SMTP, Gmail with an app password, JMAP; several accounts in one unified inbox.
- **Inbox:** conversations that unfold in the list, categories without AI, snooze, pins, tasks,
  reminders when nobody replies, flagged view, an attachments view across accounts, local rules;
  views in the sidebar you choose yourself.
- **Contacts:** every address becomes a contact, with your own pictures, search and "mark as spam";
  contact groups for the composer, vCard import and export.
- **Safe reading:** sanitised HTML in a sandbox, remote images blocked until allowed, tracking
  pixels counted and never loaded, tracking parameters removed from links, every link checked;
  images and previews decoded in an isolated helper process.
- **Writing:** identities, signatures, templates, undo send, send later, redirect, edit as new, a
  reminder when the text mentions an attachment that is missing.
- **Moving in:** take over accounts, signatures and local folders from Thunderbird (never its
  passwords); folders as mbox files in and out; the source of any message.
- **Search:** offline full-text search with operators.
- **Settings sync:** encrypted, through a folder in your own mailbox.
- **AI, optional and off by default:** a local MCP server for your own AI client, or AI in the app
  with a provider you choose.

Details: [CHANGELOG.md](CHANGELOG.md).

## Install

Download the latest release from the [releases page](https://github.com/othervice/vice-mail/releases)
(macOS on Apple silicon, Windows 11 x64). The builds are not code-signed yet;
[docs/INSTALL.md](docs/INSTALL.md) explains the warnings.

## Build

```sh
git clone https://github.com/othervice/vice-mail.git && cd vice-mail
pnpm --dir app install --frozen-lockfile
pnpm --dir app tauri dev     # run in development mode
scripts/check.sh             # all checks and tests
```

Requirements and installers: [docs/SETUP.md](docs/SETUP.md). Contributing:
[CONTRIBUTING.md](CONTRIBUTING.md). Security reports: [SECURITY.md](SECURITY.md). More documents:
[docs/README.md](docs/README.md).

## Licence

GPL-3.0-or-later, see [LICENSE](LICENSE). Copyright © 2026 BITWATER LLC.
