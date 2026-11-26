# Privacy

What Vice Mail sends where, what it keeps on your computer, and what it does not do. This describes
the code in this repository; the technical rules behind it are under "Security boundaries" in
[ARCHITECTURE.md](ARCHITECTURE.md). Adding a new kind of connection needs a recorded decision and a
change to this page.

## In short

- The app talks to **the mail servers you set up**. Everything else is optional, off until you
  turn it on, or happens only when you click something.
- There is **no telemetry**, no usage statistics, no crash reports, no advertising, no account with
  us and no server of ours. The project does not learn that you use the app.
- Your mail, the search index and your settings are stored **on your computer, not encrypted by the
  app**. Disk encryption (FileVault on macOS, BitLocker or device encryption on Windows) protects
  them when the computer is off or the disk is taken out.
- Passwords and keys are stored **only in the system keychain** (macOS) or **Credential Manager**
  (Windows), never in the app's files or logs.

## Connections the app makes

| What | When | To whom | What they learn |
|---|---|---|---|
| **Your mail servers** (IMAP, SMTP) | while the app runs and an account is set up | the servers in the account settings | everything a mail server sees from any mail program: your login, which mail you fetch, what you send. The app fetches message bodies without marking them read |
| **Finding server settings** | only during account setup, after you click "Search settings online" | your system's DNS servers (SRV records for your address's domain), then `autoconfig.<your domain>` and `<your domain>/.well-known/autoconfig/` over HTTPS | your DNS servers: your IP address and the domain of the address you are adding. `autoconfig.<your domain>`: your IP address and the full address you are adding (sent as `?emailaddress=`, as Thunderbird does, so the provider can answer per address). `<your domain>/.well-known/autoconfig/`: your IP address and the domain only. No third-party directory (such as Mozilla's ISPDB) is asked |
| **Sign-in with Microsoft or Google** | only for accounts that sign in this way: when you click "Sign in with Microsoft/Google" or "Sign in again", then while the account runs (about once an hour, to renew the access) and when you remove a Google account | your **browser** opens the provider's sign-in page (`login.microsoftonline.com`, `accounts.google.com`); the app itself talks to the provider's token service (`login.microsoftonline.com`, `oauth2.googleapis.com`) | what any sign-in tells them: your account, your IP address, the time, and that this app has access to your mail (they show it in your account's app permissions). The app sends its public app ID and, for Google, the app's public "client secret"; never your password, which it never sees. While you sign in, the app listens on your own computer (`127.0.0.1`) for the browser's return; nothing outside your computer can reach it |
| **Remote images in a message** | only after you allow them for this message, or always for a sender | the servers named in the message | your IP address, the time, and that you opened this message: remote images are often used to track exactly that. The app sends no cookies and no referrer, identifies itself only as `Mozilla/5.0`, and never connects to addresses in your local network |
| **One-click unsubscribe** | only when you click "Unsubscribe" and the message offers one-click unsubscribe (RFC 8058) | the HTTPS address in the message's `List-Unsubscribe` header | your IP address and that you unsubscribed. Other kinds of unsubscribe open a draft (mail) or your browser (web page) instead |
| **AI provider** (optional) | only if you turn on AI in the app and use an AI feature; a cloud provider only after you agreed for that provider, and only with mail from accounts you allowed for cloud use | the address in your AI profile: a local server such as Ollama or LM Studio, or a cloud provider with your own API key | the text needed for the request: for example the current conversation without repeated quotes and without attachments. Never your whole mailbox without a confirmation that states how many messages. Before the first transfer the app shows its size. What a cloud provider does with it is governed by that provider's terms |
| **Update check** (optional) | only if you turn on "Check for updates automatically" (off by default; asked once, on the first start or, if you skipped that part, after your first account), then at most once a day; or when you click "Check now" | `github.com` and GitHub's file hosts `objects.githubusercontent.com` and `release-assets.githubusercontent.com` | your IP address and the time; when you download an update, which file. The request contains no app version, no identifier and no account data. Updates are downloaded and installed only when you click |

**Not by the app, but worth knowing:**

- Links and attachments are opened by your browser or the program the system chooses, after you
  click and after the app has shown you the real link target. Those programs make their own
  connections.
- The operating system may check server certificates for revocation (OCSP, CRL) with the issuing
  certificate authority. This happens in the system, not in the app.
- On Windows, the installer downloads the Microsoft Edge WebView2 runtime from Microsoft if it is
  missing (Windows 11 normally has it). The runtime is a system component that Windows keeps up to
  date. Whether it contacts Microsoft on its own while the app runs has not been examined yet.
- The local MCP server (see below) makes no network connections at all.

Nothing else: no fonts, scripts or images from the internet for the interface itself (its Content
Security Policy forbids remote sources), and no lookups in third-party directories without a click.

## What is stored on your computer

| What | Encrypted by the app? |
|---|---|
| The database: accounts and server settings (without passwords), folders, message headers (senders, recipients, subjects, dates), previews, flags, the full-text search index, threads, contacts, drafts, signatures, templates, rules, saved searches, snoozes, pins, settings, the state of the settings sync | no |
| Message bodies and attachments you have opened or the app has prefetched, stored compressed (zstd) as files | no |
| Copies of attachments you added to a draft, and copies of attachments you opened with another program | no |
| Backups of the database taken before each update of its format (the last three) | no |
| Logs: technical events such as account IDs, server and link host names, error codes. Passwords are masked by design; the logs are not meant to contain message text, and a failed command is logged with its error code, not its error text. One file per day, the last 14 are kept | no |
| The AI request log (only if you use AI in the app): feature, provider host, model, number of messages and bytes, result, duration. No message content. Kept 30 days, at most 10,000 entries | no |
| The learning spam filter (only if you turn it on in Settings → Inbox): for every word and sign it learned from mail you marked as spam or not spam (or let it learn from your Junk folders and read inbox mail), a number standing for it (a hash, not the word itself) and how many spam and other mails had it; per learned mail which numbers it taught; and for new mail it judged, its mark and score. It stays on this device, never goes through the settings sync, and never leaves the computer. "Forget what was learned" deletes it; the learned counts are not removed with an account | no |
| The MCP access log (only if you turn on the MCP server): which client used which tool, when, on how many messages, and the outcome. Never the request's arguments or any content. Kept 30 days, at most 10,000 entries | no |
| Web view data (the interface's local storage and cache) | no |

Where these folders are on macOS and Windows, and how to remove them, is listed in
[INSTALL.md](INSTALL.md) ("Where the app keeps its data" and "Uninstall"). Removing an account in
the app deletes the mail the app stored for it and its password; your mail on the server is not
touched.

The app does not encrypt its local data because the operating system's disk encryption already
does that without a second password. Anyone who can use your user account on the unlocked
computer can read your stored mail, as they can with Apple Mail or Outlook.

## What is in the keychain

All entries are under the service name `org.vicemail.desktop`. On **Windows** each is a generic
credential of its own in Credential Manager. On **macOS** (since 0.1.3, decision 0075) they are all
inside **one** item of the login keychain, an application password named `vault` (or `vault.2`,
`vault.3`, … after changes): its content is a list of the entries below with their secrets, so an
update of the app makes macOS ask once instead of once per entry. Older versions kept one item
per entry; the first start of 0.1.3 moves them into the vault and deletes them where macOS allows.

| Entry | Holds |
|---|---|
| the account's ID (a UUID) | the account password |
| `oauth:microsoft:<id>`, `oauth:google:<id>` (and `…#1`, `…#2` for a long one) | the refresh token of an account that signs in with Microsoft or Google (the access token is kept only in memory) |
| `ai-profile:<id>` | the API key of an AI provider profile |
| `mcp-client:<id>` | the key an AI client uses to prove itself to the app's MCP server |
| `settings-sync:key` | the settings sync key (the passphrase itself is never stored) |

Keychain entries are not part of the settings sync: every device keeps its own. Since 0.1.3 a
new device can ask for passwords on purpose ("Take over passwords from another device", see
"Settings sync" below); nothing goes along on its own.

## Settings sync

If you turn on the settings sync, the app writes encrypted entries to a hidden folder
(`Settings-Sync`) in a mailbox you choose, on your own mail server. They are encrypted with
[age](https://age-encryption.org) using a key protected by your passphrase. Passwords, API keys and
other secrets are never part of them. Your mail server can see how many entries there are, their
size and when they change, but not what they contain. Details: [SYNC.md](SYNC.md).

**Passwords from another device** (since 0.1.3): a device that joined the sync can ask another one
for the passwords of accounts it took over. Both show the same six-digit code; only after you
confirm on the device that has the passwords does it send them, encrypted a second time to a key
that exists only in the new device's memory, and only for accounts whose servers are the same on
both devices. Accounts that sign in with Microsoft or Google sign in on each device. The exchange
lies in the sync folder for at most 15 minutes (usually seconds) and is deleted afterwards; your
mail server sees four more encrypted entries and their times, not what they hold.

## The MCP server for AI clients

Off until you turn it on in **Settings → AI over MCP**. When it is on:

- It accepts connections **only from this computer**: through a Unix socket that only your user
  can open (macOS) or a named pipe limited to your user (Windows), never over TCP. Each AI client
  must be added in the app first and proves itself with its own key.
- It shows **only the accounts you share**, each set to hidden (the default), read, or read and
  create drafts.
- It gives clients cleaned-up text: no raw HTML, no remote images, attachments only by name, with
  limits on size and number. Fetching a message does not mark it read.
- No tool can send, delete, move or mark mail or change settings. Drafts that a client creates
  appear in the app for you to review and send yourself.
- Every access is listed in the app; you can revoke a single client or turn the server off.

What an AI client does with the mail it reads is outside the app. A client that uses a cloud model
sends what it reads to that model's provider.

## Diagnostics

**Settings → About → Export diagnostics…** writes one file where you choose in the system's save
dialog. The app never sends it anywhere: you decide whether to attach it to an issue or an email.
It is plain text (UTF-8) rather than JSON, so you can read every line before you pass it on.

What the file contains:

- the app's version, commit and build date; the operating system, its version, the processor
  architecture, the language the system reports, the app's language setting and the web view's
  version;
- the size of the app's data directory (its path with your home folder shown as `~`) and of its
  top-level parts (database, message bodies, backups, …);
- the database's schema version, size and the number of rows per table, outbox operations per
  state and the error codes of failed ones, the state of the search index;
- per account: its random ID, the provider kind (`gmail`, `icloud`, `outlook`, … when the IMAP
  server is one of the built-in providers, `local` for a server on this computer, otherwise
  `other`), the sign-in kind (password), the connection security and port of both servers,
  whether writing is on, the sync state with the last error code and times, and the number of
  folders, messages and stored bodies;
- the state of the settings sync, updates, AI in the app and AI over MCP (on or off, counts,
  error codes);
- settings that are not personal (theme, text size, prefetch, send delay, notification switches
  and similar);
- the end of the newest three log files, at most 256 KiB of each.

What the file never contains: passwords, keys, tokens or other sign-in data (the export does not
read the keychain at all), email addresses, names, user names, the host names of your own
servers, subjects, message text, attachment names, contacts and folder names. Server host names
of the built-in providers (such as `imap.gmail.com`) and of the update source (`github.com`) may
appear in log lines, as they say nothing about you beyond the provider kind.

The log lines are checked once more before they go into the file, although the app is built not
to log personal data. Replaced are: the addresses, names, user names and server names of your
accounts and identities and your folder names wherever they appear; anything that looks like an
email address, a host name or an IPv4 address other than this computer; file paths, shown as `~/…`
when they lie in your home folder and as `<path>` otherwise, without the file name (it can be an
attachment's name); values of fields that hold secrets or personal data (`password=`, `token:`,
`subject=`, …); the credential after `Bearer` or `Basic`; known token formats and long
random-looking strings. The placeholders say what was there: `<email>`, `<host>`, `<ip>`, `~/…`,
`<path>`, `<folder>`, `<user>`, `<user name>`, `<removed>`, `[redacted]`. This check errs on the
side of removing too much. It works by patterns and by what the app knows about your accounts, so
it cannot recognise free text such as a subject or a contact's name; the app does not write such
text to its logs.

## Questions

Questions about privacy can be asked in a public issue. Suspected security problems go through
[SECURITY.md](../SECURITY.md).
