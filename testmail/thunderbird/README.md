# Thunderbird fixture profile

A made-up Thunderbird profile directory for the tests of the takeover from Thunderbird
(stage 19): `profiles.ini` as Thunderbird keeps it (macOS `~/Library/Thunderbird/`, Windows
`%APPDATA%\Thunderbird\`), two profiles, accounts with servers, identities and signatures in
`prefs.js`, and local folders as mbox files under `Mail/Local Folders`.

It holds no real data and no passwords. The servers are the local test server
(`testmail/accounts.env`); the addresses are the test accounts and invented people under
`vicemail.test`. `logins.json` and `key4.db` (where Thunderbird keeps saved passwords) are decoys
with one line of text: Vice Mail never reads them, and the tests make them unreadable to prove it.

The mbox files use Thunderbird's format (`From - <date>` separators, `X-Mozilla-Status` fields,
`>From ` quoting). The third message of `Inbox` is marked deleted (`X-Mozilla-Status: 0009`) and
must not be imported.
