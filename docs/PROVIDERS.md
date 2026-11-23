# Mail providers

What Vice Mail supports per provider, and what it does not. "Tested" means: covered by automated
tests in this repository. Where only a check on a real account can prove something, this page says
so. Last updated 23 November 2026 (stage 14, blocks A to C; decisions
`docs/decisions/0034-gmail-ueber-imap.md` and `docs/decisions/0042-anmeldung-mit-microsoft-und-google.md`).

Vice Mail speaks IMAP and SMTP, with a password (or an app password) or, for Microsoft and Google,
with the provider's own sign-in (OAuth 2.0). It never talks to servers other than those of the
accounts you set up and, while you sign in, the provider's sign-in service.

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
| Outlook.com, Hotmail, Microsoft 365 | "Sign in with Microsoft" (OAuth 2.0 in the system browser), IMAP and SMTP with the access token | **built, not yet usable**: needs the project's app registration at Microsoft (client ID) in the build; the whole sign-in is tested against a local OAuth server, not against Microsoft |
| Gmail with "Sign in with Google" | OAuth 2.0 instead of an app password, IMAP and SMTP with the access token | **built, not yet usable**: needs the project's Google Cloud client in the build and, beyond 100 test users, Google's verification; tested against a local OAuth server, not against Google |
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

**Sign in with Google** instead of an app password: built (block C), but not usable in current
builds; see "Sign-in with Microsoft and Google" below.

**Not supported on Gmail:** the Gmail API, labels on list rows and
for several messages at once, renaming or deleting labels in the app, Gmail's own conversation ids
(`X-GM-THRID`). Old settings sync records stay in Gmail's "All Mail" (visible in the web interface).

## iCloud Mail

Use an app-specific password from account.apple.com. The IMAP user name is the part before the @,
the SMTP user name the full address; the app fills both in. Not tested with a real account.

## Yahoo Mail

Use an app password from the Yahoo account security page. Not tested with a real account.

## Sign-in with Microsoft and Google (OAuth 2.0)

**How it works.** For these providers the account setup shows "Sign in with Microsoft" (instead of
a password field: Microsoft accepts no passwords for mail apps) or "Or sign in with Google" (next to
the app password). The app opens the provider's sign-in page in your **system browser**, never inside
the app, and waits up to five minutes; you can cancel the wait. You sign in on the provider's page
and allow access to your mail; the app never sees your provider password. The browser then returns
to a one-time address on your own computer (`http://localhost:<port>` for Microsoft,
`http://127.0.0.1:<port>/` for Google), where the app accepts exactly the answer that belongs to
this sign-in (authorization code with PKCE, RFC 7636, and a random `state`; RFC 8252). The app logs
in to IMAP with the access token (`XOAUTH2`, or `OAUTHBEARER` where a server offers only that) and
adds the account read-only, like every account.

**What is stored.** The refresh token, in the system keychain (macOS keychain, Windows Credential
Manager); the short-lived access token only in memory. The app renews the access token before it
runs out and once more if a server rejects it. If the provider says the permission has expired or
was withdrawn, the account stops syncing and its settings show "Sign in again". Tokens are never
logged, never in the database or a diagnostics file, never in the settings sync: the synced account
names only its provider, and another computer that gets the account through the settings sync shows
"Sign in again" and signs in there itself. A token is only ever sent to
the provider's own IMAP and SMTP servers. Removing the account deletes the tokens; at Google the app
also revokes them. Microsoft offers no revocation to apps like this one: the permission ends with its
lifetime, or you remove the app in your Microsoft account.

**Servers.** Microsoft: `outlook.office365.com` (IMAP, port 993), `smtp-mail.outlook.com`
(Outlook.com) or `smtp.office365.com` (Microsoft 365), port 587 with STARTTLS. Google:
`imap.gmail.com`, `smtp.gmail.com`. Microsoft 365 and Google Workspace domains are recognised by
these servers after the online search or when you type them in.

**Availability.** Each build needs the project's registration at the provider: a client ID for
Microsoft (Microsoft Entra, public client, no secret) and a client ID with its "client secret" for
Google (a "Desktop app" client; Google states that this secret is not confidential in installed
apps). A build without them says that it cannot sign in with Microsoft, and offers only the app
password for Gmail. As of 23 November 2026 the project has **not registered** either app yet, so
current builds cannot sign in with Microsoft or Google.

**Tested:** the complete sign-in against Stalwart, the local test server, which is an OAuth 2.0
authorization server too: PKCE, `state`, forged answers, wrong paths and ports, time limit, cancel,
refusal, renewal before expiry and after a rejected login, refresh token rotation, revocation,
`XOAUTH2` and `OAUTHBEARER` for IMAP, `XOAUTH2` for SMTP (logging in and sending), read-only
protection; the Microsoft and Google profiles against a mock authorization server.
**Not tested:** real Microsoft and Google accounts (needs the registrations), Windows.

**Microsoft Graph and the Gmail API** are not used. IMAP and SMTP with OAuth cover what the app
does; Graph would only be needed where a Microsoft 365 administrator has turned off IMAP or SMTP
AUTH for the organisation, which the app then reports as a failed login.

## Google's verification

`https://mail.google.com/` (full IMAP and SMTP access) is a **restricted scope**. What that means for
"Sign in with Google":

- **Without verification** (the Google Cloud project's consent screen in the "Testing" status), only
  the test users entered in the project can sign in, at most **100**, and Google shows them a warning
  that the app is not verified. Refresh tokens of apps in "Testing" expire after **7 days**, after
  which the account shows "Sign in again". This is enough for the project's own tests.
- **For everybody** the consent screen must be "In production" and **verified** by Google:
  - a verified **domain** (Google Search Console) for the app's home page and the links below,
  - a public **home page** describing the app, and a **privacy policy** on that domain that says
    what the app does with Gmail data (Vice Mail keeps mail on the user's computer and sends it
    nowhere else),
  - the app's name and logo matching the home page, a support address,
  - a **demo video** (YouTube, unlisted is fine) that shows the sign-in, the consent screen with the
    requested scope, and how the app uses the data,
  - a justification why a narrower scope does not suffice (IMAP and SMTP need
    `https://mail.google.com/`),
  - for restricted scopes in addition a yearly **security assessment** (CASA, by an assessor Google
    accepts; tier and cost depend on the assessor). Apps that store restricted data only on the
    user's device may qualify for a lighter assessment; Google decides.
  - Verification typically takes several weeks; changes to scopes or branding need it again.
- Until then Gmail works with an **app password** (above).

## Outlook.com and Microsoft 365

Microsoft only accepts sign-in through its own sign-in page (OAuth), not a password; see "Sign-in
with Microsoft and Google" above. The app knows the servers, shows "Sign in with Microsoft" in the
account setup and has no password field for these accounts. For a work or school account the
organisation may require an administrator to approve the app first; the sign-in then ends with
"The provider did not allow the sign-in".
