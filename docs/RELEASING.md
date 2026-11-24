# Releasing Vice Mail

How a release is built, signed for the in-app updater and published, how the update signing
key is kept, backed up and replaced, and how the macOS build is signed with a Developer ID and
notarised. Background: decisions 0035 and 0037 (`docs/decisions/`).

> Until the first public release (stage 15) the repository is private and there is no
> `latest.json` on GitHub; "Check now" in the app then reports that it could not check.

## The update signing key

The app accepts an update only if its file is signed with the project's update key **and** the
signature names the version the manifest announces (`requireSignedVersion`). Installed copies
trust exactly one public key, the one in `app/src-tauri/tauri.conf.json` →
`plugins.updater.pubkey` (minisign, key ID `CBF2BA7119C9327E`).

### Where it lives

On the build Mac, in the **login keychain**, as three generic passwords of the service
`org.vicemail.updater`:

| Account | Contents |
|---|---|
| `private-key` | the private key as `tauri signer` writes it (base64, encrypted with the password) |
| `password` | the password of the private key (random, 256 bit) |
| `public-key` | the public key, the same as in `tauri.conf.json` |

Nowhere else: not in the repository, not in a file, not in an environment file, not in a log.
The build scripts read it only when `VICEMAIL_UPDATER=1` is set (`scripts/updater-signing.sh`),
and only into the environment of that build. `scripts/updater-key.sh check` confirms that both
secret items are readable and that the public key matches `tauri.conf.json`.

### Back it up now

**If the private key or its password is lost, no installed copy can ever be updated again.**
Every user would have to download and install the next version by hand. Back up both items once,
right after they were made, and again after every rotation:

1. Open **Keychain Access** (*Schlüsselbundverwaltung*), select the **login** keychain and search
   for `org.vicemail.updater`. There are three entries.
2. For `private-key` and for `password`: double-click the entry, tick **Show password**, confirm
   with the Mac's password, and copy the value into a password manager entry of its own (for
   example "Vice Mail update key" with the key as a note and the password as the password).
   The private key is useless without the password and the other way round; keep both.
3. Keep the backup somewhere other than this Mac (the password manager's sync, or an encrypted
   USB stick in a drawer). Never put it in a repository, a chat or an email.

To restore it on a new build Mac, put the two values back with Keychain Access (**File → New
Password Item**, name `org.vicemail.updater`, account `private-key` or `password`) and add the
public key the same way (account `public-key`), then run `scripts/updater-key.sh check`.

### Making a key (done once, 23.11.2026)

```sh
scripts/updater-key.sh generate     # refuses if a key is stored already
```

It makes the key pair with `tauri signer generate` through the CLI's in-process Node API (the
password never appears on a command line), stores the three items and prints the public key,
which goes into `plugins.updater.pubkey`.

### Rotating the key

Rotate only if the private key may have leaked, or before it is lost (for example when the build
Mac is replaced and the backup is doubtful). Installed copies trust only the old public key, so a
rotation needs one bridge release. The scripts take `VICEMAIL_UPDATER_SERVICE` (which keychain
service to use) and `VICEMAIL_UPDATER_PUBKEY` (which public key the signature check expects) for
this:

1. Make the new key next to the old one and back it up:
   `VICEMAIL_UPDATER_SERVICE=org.vicemail.updater.next scripts/updater-key.sh generate`.
2. **Bridge release:** put the **new** public key into `plugins.updater.pubkey`, and sign the
   release with the **old** key, checking against the old public key:

   ```sh
   VICEMAIL_UPDATER_PUBKEY="$(scripts/updater-key.sh public)" \
     VICEMAIL_UPDATER=1 scripts/release-local.sh <version>
   ```

   Installed copies accept it (old key) and from then on trust the new key.
3. Every later release is signed with the new key:
   `VICEMAIL_UPDATER_SERVICE=org.vicemail.updater.next VICEMAIL_UPDATER=1 scripts/release-local.sh …`,
   or move the new values to `org.vicemail.updater` in Keychain Access once the old items are
   deleted. Delete the old items only after the bridge release has been out long enough.
4. If the old key leaked, say so in the release notes: until people have the bridge release,
   whoever has the old key could sign an update they would accept.

If the old key is **lost** (not leaked), there is no bridge: publish the next version with a new
key and tell users in the release notes and `docs/INSTALL.md` to install it by hand once.

## macOS signing and notarisation

A macOS build signed with an Apple **Developer ID Application** certificate and notarised by Apple
opens on any Mac without the Gatekeeper workaround of `docs/INSTALL.md`. Two things on the build
Mac make `scripts/build-macos.sh` (and so `scripts/release-local.sh`) produce such a build:

| What | Where | Made by |
|---|---|---|
| the signing identity: private key plus Apple's certificate "Developer ID Application: *name* (*team ID*)" | login keychain | `scripts/apple-signing-setup.sh` steps 1 and 2 |
| the notarisation credentials, as the notarytool profile `vicemail-notary` (an App Store Connect API key, or an Apple Account with an app-specific password) | login keychain | `scripts/apple-signing-setup.sh` step 3 |

Without the identity the build stays ad-hoc signed and says so. Windows stays unsigned for now
(decision 0035).

> **Status (24.11.2026):** prepared. The membership is to run as the organisation **BITWATER LLC**,
> the publisher of Vice Mail (decision 0035, addendum of 24.11.2026), so the certificate reads
> "Developer ID Application: BITWATER LLC (TEAM ID)". The signing path was tested with a self-signed
> test certificate (hardened runtime, helper, disk image, update archive, start from the disk
> image); notarisation, stapling and Gatekeeper's verdict can only be tried with the real
> certificate. The first signed release is planned as 0.1.1.

### Enrolling as an organisation (once)

Apple needs for an organisation: the legal name exactly as in the D-U-N-S record (**BITWATER LLC**,
D-U-N-S **146941332**), a website on the organisation's domain, a work email on that domain, and
an Apple Account with two-factor authentication of someone who may sign contracts for the company.
Enrol at <https://developer.apple.com/programs/enroll/> as *Company / Organization*. An enrolment
that was started as an individual is not converted by itself: ask Apple Developer Support to
change it to the organisation, or cancel it and enrol again. Apple may call to verify; the
membership costs 99 USD a year.

### After the membership is active (once)

All steps work over SSH; the browser steps run on any computer. Only the **Account Holder** of the
membership can create Developer ID certificates.

1. **On the build Mac:** unlock the login keychain if you are connected over SSH, then start the
   guided setup:

   ```sh
   security unlock-keychain ~/Library/Keychains/login.keychain-db
   scripts/apple-signing-setup.sh
   ```

   Step 1 asks for your email address, name and country, makes an RSA 2048 private key directly in
   the login keychain (the key never touches the disk) and writes the certificate signing request
   `~/Downloads/ViceMail-DeveloperID.certSigningRequest`. The request is not secret; copy it to the
   computer with the browser (`scp`, or VS Code: right-click → *Download…*), or copy the text the
   script prints into a file with that name.
2. **developer.apple.com → Account → Certificates, IDs & Profiles → Certificates → "+"**
   (direct: <https://developer.apple.com/account/resources/certificates/add>). Under *Software*
   choose **Developer ID Application**, *Continue*. Profile type **G2 Sub-CA (Xcode 11.4.1 or
   later)**, choose the `.certSigningRequest` file, *Continue*, **Download**. The file is called
   `developerID_application.cer`.
3. **On the build Mac:** copy the `.cer` there (it is not secret either) and import it:

   ```sh
   scripts/apple-signing-setup.sh cert ~/Downloads/developerID_application.cer
   ```

   The script checks that the certificate belongs to the key from step 1, imports it, fetches
   Apple's intermediate certificate if macOS lacks it, and makes a test signature.
4. **appstoreconnect.apple.com → Users and Access → Integrations → App Store Connect API →
   Team Keys** (direct: <https://appstoreconnect.apple.com/access/integrations/api>). The first time,
   the Account Holder clicks **Request Access** and accepts the terms. Then **"+"** (*Generate API
   Key*): name `Vice Mail notarisation`, access **Developer**, *Generate*. Note the **Issuer ID**
   (above the table) and the **Key ID**, and **Download API Key**: `AuthKey_<KEY ID>.p8`. Apple
   offers the download **only once**; put a copy into your password manager right away. (Individual
   keys from your user profile do not work with notarytool; it has to be a team key. Should Apple
   refuse the key with 401 or 403, give it the role *App Manager*.)
5. **On the build Mac:** copy the `.p8` there (over `scp`, it is a secret) and store the profile:

   ```sh
   scripts/apple-signing-setup.sh notary
   ```

   Choose `1`, give the path, the Key ID and the Issuer ID. notarytool checks them with Apple and
   keeps its own copy of the key in the login keychain. The script then proves that the profile
   works without the file and offers to delete the `.p8` from the Mac; say yes once it is in your
   password manager (and empty the browser's download folder on the other computer).
   *Fallback without an API key:* choose `2`, enter your Apple Account and the Team ID; notarytool
   asks for an app-specific password (<https://account.apple.com> → Sign-In and Security →
   App-Specific Passwords).
6. **Check:** `scripts/apple-signing-setup.sh --check` must end with "Ready". It prints the identity,
   its expiry date, a test signature and whether Apple accepts the profile; no secrets.
7. **Signed build:** `scripts/build-macos.sh <version>` or, for a release,
   `VICEMAIL_UPDATER=1 scripts/release-local.sh <version>`. The summary must show
   `signature: Developer ID Application: … notarised and stapled` and
   `spctl: … accepted source=Notarized Developer ID`. Two notarisation rounds run (most finish
   within 15 minutes; the very first submission of a new team can take hours, see
   `VICEMAIL_NOTARY_TIMEOUT`).
8. **Try it on a second Mac:** download the disk image with a browser, open it, drag the app to
   Applications and start it. macOS should only ask whether to open an app downloaded from the
   internet; no "Open Anyway" in System Settings. Then update `docs/INSTALL.md` (status section)
   and decision 0035.

### What the build does with an identity

1. Tauri lays out the app as before (sealed ad hoc). The script then signs inside out: first
   `Contents/MacOS/mail-mcp` (identifier `org.vicemail.desktop.mail-mcp`, no entitlements), then the
   app, both with the hardened runtime (`--options runtime`) and a secure timestamp
   (`--timestamp`), the app with `app/src-tauri/Entitlements.plist`. That file is empty on purpose;
   it explains why WKWebView, the keychain and outgoing connections need no entitlement in a
   non-sandboxed app.
2. Round 1: a zip of the app goes to `notarytool submit --keychain-profile vicemail-notary --wait`;
   the ticket is stapled to the app.
3. The disk image is made from the stapled app (same layout as Tauri's: app, Applications link,
   volume icon), signed, notarised in round 2 and stapled.
4. With `VICEMAIL_UPDATER=1`, the update archive is packed from the stapled app and signed with the
   update key for this version (`tauri signer sign --app-version`), so an in-app update installs
   the notarised app.
5. Checks: `codesign --verify --deep --strict`, hardened runtime, timestamp and team ID on app and
   helper, `spctl -a -vv` on the app (`-t exec`) and the disk image (`-t open`) must say
   `accepted` and `source=Notarized Developer ID`, `xcrun stapler validate` on app, disk image,
   the app inside the image and the app inside the update archive, the same code (CDHash) in all
   three, then the usual start from the disk image. Apple's notarisation logs land in
   `target/release/bundle/notarisation-*.json` (read them: they may hold warnings).

Any failure stops the build. It never falls back to an ad-hoc build once an identity was found;
to build unsigned on purpose, set `VICEMAIL_SIGNING_IDENTITY=-`.

| Variable | Effect |
|---|---|
| `VICEMAIL_SIGNING_IDENTITY` | the identity to use, by name or SHA-1 (default: the one valid "Developer ID Application" identity; with two, for example after a renewal, the build asks for this); `-` builds ad hoc |
| `VICEMAIL_NOTARY_PROFILE` | another notarytool profile (default `vicemail-notary`) |
| `VICEMAIL_NOTARY_TIMEOUT` | how long to wait for each notarisation (default `2h`); after a timeout, `xcrun notarytool info <id> --keychain-profile vicemail-notary` shows the state, and the build is run again |
| `VICEMAIL_SIGNING_KEYCHAIN` | look for the identity only in this keychain file (tests, later CI) |
| `VICEMAIL_SIGNING_TEST=1` | allows an identity that is not an Apple Developer ID (a self-signed test certificate): signed and checked like a release, **never notarised, never to be published** |

### Keychain prompts after the switch

The app's account passwords live in the login keychain, and macOS lets the app that stored an item
read it without asking. An ad-hoc signature changes with every build, a Developer ID signature
stays the same across versions (identifier and team ID). So on the first start of the first signed
version, macOS is expected to ask once per account whether "Vice Mail" may use its stored password
(answer with the Mac's password and **Always Allow**); later signed updates should not ask again.
Not yet observed; check it with the first signed build.

### Renewal

- A **Developer ID Application certificate is valid for five years.** Apps signed while it was
  valid keep opening after it expires (the secure timestamp proves when they were signed); only new
  builds need a valid certificate. `scripts/apple-signing-setup.sh --check` warns 60 days ahead.
  To renew: `scripts/apple-signing-setup.sh csr --new`, steps 2 and 3 above, then delete the old
  identity in Keychain Access (login → My Certificates) so that only one valid identity is left.
  The team ID stays, so the app's identity for macOS (and keychain access) stays too.
- The **membership** renews every year (99 USD). Without it, no new certificates and no new
  notarisations. Builds published before stay signed and notarised (their timestamp and ticket do
  not depend on the membership); confirm with Apple's current terms if it ever matters.
- The **API key** does not expire. Revoke it in App Store Connect (same page as step 4) if it may
  have leaked, make a new one and run `scripts/apple-signing-setup.sh notary` again.

### What to back up

| Item | How | If lost |
|---|---|---|
| `AuthKey_<KEY ID>.p8`, with Key ID and Issuer ID | password manager, right after the download (Apple offers it once) | revoke it, make a new key, `apple-signing-setup.sh notary` |
| the signing identity (private key and certificate) | Keychain Access → login → My Certificates → right-click "Developer ID Application: …" → *Export* → `.p12` with a strong password, into the password manager or an encrypted stick | make a new certificate (steps 1 to 3); published builds are not affected |
| the notarytool profile | nothing to back up; it is made again from the `.p8` | `apple-signing-setup.sh notary` |

On a new build Mac: import the `.p12` with Keychain Access (or `security import <file>.p12 -k
~/Library/Keychains/login.keychain-db -T /usr/bin/codesign`, which asks for its password), run
`scripts/apple-signing-setup.sh notary` with the `.p8`, then `--check`. If the identity's private key
may have leaked, revoke the certificate in the portal (Certificates → the certificate → *Revoke*)
after reading Apple's notes there: a revocation can also stop published builds from opening.

## CI

`.github/workflows/ci.yml` runs on GitHub's hosted runners, on macOS and on Windows
(`macos-latest`, `windows-latest`): `scripts/check.sh` on every push to `main` and every pull
request, both installers on tags `v*` and on a manual start ("Run workflow"), unsigned and without
update signature, kept as run artifacts for 14 days. Publishing stays the manual step below.
On Windows the installer is built natively, the MCP helper first, as `scripts/build-windows.sh`
does it with cargo-xwin on the Mac.

Before the move to GitHub (23–24 Nov 2026) a Gitea Actions runner on the build Mac did the same
for the private repository. It is retired: the workflow file is gone, Actions are off for the Gitea
repository, and the runner is stopped (`brew services stop gitea-runner`; its configuration is in
`/opt/homebrew/etc/gitea-runner/`). Pull requests from others must never run on the build Mac,
which can reach the maintainer's keychain.

## Building a release

On the build Mac (Apple silicon), from a clean checkout of the release commit:

```sh
VICEMAIL_UPDATER=1 VICEMAIL_RELEASE_NOTES=notes.txt scripts/release-local.sh 0.2.0
```

- `scripts/build-macos.sh` and `scripts/build-windows.sh` run with `createUpdaterArtifacts`; the
  Tauri CLI signs the macOS `.app.tar.gz` and the Windows `-setup.exe` and binds each signature to
  the version. Each script checks the new signature against `plugins.updater.pubkey` with
  `scripts/updater-manifest.mjs verify` and stops if it does not verify.
- With a Developer ID identity and the notarytool profile in the keychain, the macOS build is
  signed, notarised and stapled, and the update archive holds the notarised app (section above).
  The build stops if notarisation fails. The Windows installer stays unsigned.
- Over SSH the login keychain may be locked; unlock it first:
  `security unlock-keychain ~/Library/Keychains/login.keychain-db`.
- `notes.txt` is plain text (no Markdown or HTML; the app shows it as it is, at most 4,000
  characters).

`dist/<version>/` then holds:

| File | Purpose |
|---|---|
| `Vice-Mail-<version>-macos-arm64.dmg` | manual installation on macOS |
| `Vice-Mail-<version>-windows-x64-setup.exe` | manual installation on Windows, and the Windows update |
| `Vice-Mail-<version>-windows-x64-setup.exe.sig` | its update signature |
| `Vice-Mail-<version>-macos-arm64.app.tar.gz` | the macOS update (the app bundle) |
| `Vice-Mail-<version>-macos-arm64.app.tar.gz.sig` | its update signature |
| `latest.json` | the update manifest |
| `SHA256SUMS` | checksums of all of them |

Without `VICEMAIL_UPDATER=1` the scripts build only the `.dmg`, the `.exe` and `SHA256SUMS`, and
need no key.

## Publishing

1. Create the GitHub release with the tag `v<version>` (for example `v0.2.0`) in the public
   repository and upload **every** file from `dist/<version>/` with its name unchanged. The
   manifest points to `https://github.com/othervice/vice-mail/releases/download/v<version>/<file>`.
2. Mark it as the latest release (not as a pre-release): the app reads
   `https://github.com/othervice/vice-mail/releases/latest/download/latest.json`, the endpoint in
   `tauri.conf.json`, and GitHub serves "latest" only from the newest full release.
3. Check from a browser that the manifest URL answers with the new `latest.json`.
4. Keep the previous release downloadable: it is the way back for anyone who needs it.

If the repository name changes, edit only `plugins.updater.endpoints` in `tauri.conf.json`;
`scripts/release-local.sh` derives the download addresses from it (or set
`VICEMAIL_UPDATE_BASE_URL`).

## What the app does with it

Nothing, unless the user turned on "Check for updates automatically" (asked once after the first
account is set up; off by default) or clicks "Check now" in Settings → About. Then it fetches
`latest.json` (at most once a day automatically), offers a newer version, downloads it only on
"Download update", verifies the signature, and installs it only on "Restart to install": macOS
replaces the app bundle in place and restarts; Windows runs the installer in passive mode (no
questions, no "uninstall before installing" page) and the installer starts the app again.
