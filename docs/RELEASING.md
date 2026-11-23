# Releasing Vice Mail

How a release is built, signed for the in-app updater and published, and how the update signing
key is kept, backed up and replaced. Background: decision 0037 (`docs/decisions/0037-*.md`).

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

## Building a release

On the build Mac (Apple silicon), from a clean checkout of the release commit:

```sh
VICEMAIL_UPDATER=1 VICEMAIL_RELEASE_NOTES=notes.txt scripts/release-local.sh 0.2.0
```

- `scripts/build-macos.sh` and `scripts/build-windows.sh` run with `createUpdaterArtifacts`; the
  Tauri CLI signs the macOS `.app.tar.gz` and the Windows `-setup.exe` and binds each signature to
  the version. Each script checks the new signature against `plugins.updater.pubkey` with
  `scripts/updater-manifest.mjs verify` and stops if it does not verify.
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
