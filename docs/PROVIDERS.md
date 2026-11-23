# Mail providers

What Vice Mail supports per provider, and what it does not. "Tested" means: covered by automated
tests in this repository. Where only a check on a real account can prove something, this page says
so. Last updated 23 November 2026 (stage 14, block A; decision `docs/decisions/0034-gmail-ueber-imap.md`).

Vice Mail speaks IMAP and SMTP with a password (or an app password) and nothing else for now. It
never talks to servers other than those of the accounts you set up.

## Overview

| Provider | How | Status |
|---|---|---|
| Any IMAP/SMTP server | password, TLS or STARTTLS | supported, tested against a local Stalwart server |
| Gmail (personal and Google Workspace) | IMAP and SMTP with an app password | supported; Gmail specifics tested with recorded Gmail responses; a check on a real account is pending |
| iCloud Mail | IMAP and SMTP with an app-specific password | expected to work as plain IMAP (settings and hint built in); not tested with a real account |
| Yahoo Mail | IMAP and SMTP with an app password | expected to work as plain IMAP (settings and hint built in); not tested with a real account |
| GMX, WEB.DE | IMAP and SMTP with the account password, after turning on IMAP in the web mail | expected to work as plain IMAP; not tested with a real account |
| Fastmail, Posteo, mailbox.org, Telekom, Infomaniak | IMAP and SMTP (app password where the provider asks for one) | expected to work as plain IMAP; not tested with a real account |
| Proton Mail | only through Proton Mail Bridge on the same computer | expected to work as plain IMAP; not tested |
| Outlook.com, Hotmail, Microsoft 365 | needs sign-in with Microsoft (OAuth) | **not supported yet**, planned (stage 14, block B) |
| Gmail with "Sign in with Google" | OAuth instead of an app password | **not supported yet**, planned (stage 14, block C) |
| JMAP (Fastmail, Stalwart) | JMAP instead of IMAP | not supported, planned (stage 14, block D) |
| POP3, Exchange (EWS, MAPI) | – | not supported, not planned |

"Expected to work" means: these providers use standard IMAP and SMTP, the app knows their server
settings and shows what to do in the account setup, but nobody has tested them with a real account
yet. The generic IMAP support underneath is tested.

## Any IMAP server

Tested against Stalwart (decisions 0001, 0002, 0008, 0012): TLS and STARTTLS (never unencrypted),
LOGIN and `AUTHENTICATE PLAIN`, SPECIAL-USE folder roles, IDLE, CONDSTORE and QRESYNC, MOVE with a
fallback to COPY and EXPUNGE, UIDPLUS, custom keywords, APPEND for drafts and sent copies, server
search. What a server lacks is not faked: without IDLE new mail arrives by polling, without CONDSTORE
flags are compared window by window.

New accounts are read-only: the app opens folders with EXAMINE and refuses every writing command
before it is sent. You allow changes per account in its settings.

A copy of each sent message goes into Sent, unless you choose otherwise in the Compose settings.

## Gmail over IMAP

**Setup.** Your normal Google password does not work. Turn on 2-Step Verification in your Google
account, create an app password, and enter it as the password. IMAP is always on for personal Gmail
accounts (since January 2025); for a Google Workspace account your administrator must allow IMAP.
The app shows these hints when it recognises Gmail by the address, by the servers it finds, or by
the servers you type in (`imap.gmail.com`, `smtp.gmail.com`).

**What the app does differently on Gmail** (when the server announces `X-GM-EXT-1`):

- **Labels instead of folders.** Each label shows up as a folder, as Gmail's IMAP presents it. A
  message under several labels, in the inbox and in "All Mail" is one message (recognised by
  Gmail's `X-GM-MSGID`, also without a Message-ID header): one result in search and conversations,
  announced once as new mail. Folder counters stay per folder, as in Gmail.
- **Labels in the reading pane.** The message's labels (`X-GM-LABELS`) are shown under its header,
  including "Important" and "Starred". With changes allowed for the account you can add and remove
  labels there; the change goes to Gmail through the app's outbox (`STORE ±X-GM-LABELS`), like every
  other change, with its state visible in the activity view.
- **Archive removes the Inbox label.** No archive folder is needed and nothing is moved: the message
  stays in "All Mail" and keeps its other labels. Undo puts the Inbox label back.
- **Moving out of "All Mail"** into the inbox or a label only adds that label, so Gmail never
  treats it as a deletion.
- **No second copy in Sent.** Gmail files sent messages itself; the app adds none (setting
  "automatic" in the Compose settings; the account settings say so).
- **Settings sync records** (the sync folder is a label on Gmail) are kept out of "All Mail" in
  the app. Settings sync on Gmail has not been tried with a real account.

**Tested:** with recorded Gmail responses (the adapter's command layer fed with Gmail's lines) and
in the store, outbox and UI tests listed in decision 0034. Nothing here changes behaviour for other
servers; their server tests still run against Stalwart.

**Not yet proven on a real account:** the hand check list in decision 0034 (labels as in the web
interface, archive and undo, exactly one Sent copy, label changes with non-ASCII names, delete and
spam). Until that is done, treat the Gmail specifics as implemented but unconfirmed.

**Not supported on Gmail:** sign-in with Google (block C), the Gmail API, labels on list rows and
for several messages at once, renaming or deleting labels in the app, Gmail's own conversation ids
(`X-GM-THRID`). Old settings sync records stay in Gmail's "All Mail" (visible in the web interface).

## iCloud Mail

Use an app-specific password from account.apple.com. The IMAP user name is the part before the @,
the SMTP user name the full address; the app fills both in. Not tested with a real account.

## Yahoo Mail

Use an app password from the Yahoo account security page. Not tested with a real account.

## Outlook.com and Microsoft 365

Microsoft only accepts sign-in through its own login page (OAuth), not a password. The app knows the
servers and says so in the account setup; adding such an account fails until block B is done
(sign-in in the system browser, IMAP and SMTP with OAuth, Microsoft Graph only if IMAP does not
suffice).

## Google sign-in (planned)

Block C adds "Sign in with Google" instead of an app password. It needs a Google Cloud project and,
beyond 100 users, Google's verification (domain, privacy policy, demo video). Until then Gmail works
with an app password as described above.
