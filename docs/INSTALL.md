# Installing Vice Mail

"Vice Mail" is a working title. This guide covers the local builds made with
`scripts/release-local.sh`: a disk image for macOS and an installer for Windows.

> **Pre-alpha.** Things will break, and data formats may change between versions. Do not use it
> with a mailbox you cannot afford to lose. Every account you add starts **read-only**: the app
> downloads and shows your mail but changes nothing on the server (no read status, no moving, no
> deleting). Allowing changes is a separate step per account that you take yourself once the app
> offers it; until then the server stays as it is.

> **Signed and unsigned builds.** The Windows installer is not signed, so Windows warns before the
> first start. On macOS there are two kinds of builds: **signed** ones (Apple Developer ID,
> notarised by Apple), which open like any app from the internet, and **unsigned** ones (only
> ad-hoc signed), which macOS blocks until you allow them once. The builds so far are unsigned;
> signing is prepared and starts once the project's Apple developer membership is active. The
> release notes say which kind a release is, and "Is my copy signed?" below shows how to check.
> This guide explains each warning and what is safe to do about it.

## System requirements

| | macOS | Windows |
|---|---|---|
| Hardware | Mac with Apple silicon (M1 or later) | 64-bit PC with an Intel or AMD processor (x64) |
| System | macOS 11 Big Sur or later (the minimum the build declares) | Windows 11 |
| Web view | WebKit, part of macOS | Microsoft Edge WebView2 Runtime, part of Windows 11; the installer fetches it if it is missing |
| Space | about 15 MB for the app, plus your mail | about 25 MB for the app, plus your mail |
| Tested | macOS 27.2 only; older versions untested | not yet tested on Windows |

Not supported: Intel Macs (there is no Intel or universal build yet), Windows on Arm, Windows 10
(may work, never tested), Linux.

## Files and checksums

A release directory contains three files (`<version>` is for example `0.1.0`):

| File | For |
|---|---|
| `Vice-Mail-<version>-macos-arm64.dmg` | macOS |
| `Vice-Mail-<version>-windows-x64-setup.exe` | Windows |
| `SHA256SUMS` | the SHA-256 checksum of both |

For an unsigned build, the checksum is the only way to confirm that your copy is the file that was
built. It only helps if you get `SHA256SUMS` from the person who built the files through a channel
you trust; a checksum downloaded from the same place as the file only catches a broken download.
A signed macOS build is checked by macOS itself as well (signature and Apple's notarisation); the
Windows installer is unsigned either way.

macOS, in Terminal, in the folder with the downloaded files:

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS
```

Every file you have must be reported as `OK`.

Windows, in PowerShell:

```powershell
Get-FileHash -Algorithm SHA256 .\Vice-Mail-<version>-windows-x64-setup.exe
```

Compare the `Hash` with the line for the `.exe` in `SHA256SUMS` (PowerShell prints it in upper
case; the comparison ignores case).

If a checksum does not match, delete the file and do not open it.

## macOS

### Install

1. Double-click `Vice-Mail-<version>-macos-arm64.dmg`. A window opens with the app and a link to
   the Applications folder. (It has no background picture; the builds skip the Finder layout.)
2. Drag **Vice Mail** onto **Applications**.
3. Eject the disk image (Finder sidebar, or drag it to the Trash).

### First start and Gatekeeper

When a file arrives through a browser, Mail, Messages or AirDrop, macOS marks it with the
*quarantine* attribute (`com.apple.quarantine`). The copy you drag out of the disk image inherits
it. Before the first start of a quarantined app, Gatekeeper checks that it is signed with an Apple
Developer ID and notarised by Apple.

**Signed builds** pass that check. On the first start macOS asks once whether you want to open an
app downloaded from the internet; click **Open**. Nothing else is needed: no System Settings, no
`xattr`, no checksum required. If macOS blocks a build that the release notes call signed, do not
work around it; download it again and report it.

**Unsigned builds** are neither signed with a Developer ID nor notarised, so macOS blocks their
first start. That is expected; it does not mean that something is wrong with the file. Allow it
once as follows.

**macOS 15 Sequoia and later** (including 26 and 27):

1. Double-click the app in Applications. macOS reports that it was not opened because Apple could
   not verify that it is free of malware. Click **Done**, not **Move to Trash**.
2. Open **System Settings → Privacy & Security** (German: *Systemeinstellungen → Datenschutz &
   Sicherheit*) and scroll down to **Security**. There is a line saying that "Vice Mail" was
   blocked. Click **Open Anyway** (*Dennoch öffnen*) and follow the prompts: macOS asks once more
   whether to open the app, and for your password or Touch ID.

The button only appears for about an hour after the blocked attempt; if it is missing, start the app
once more and go back to the settings.

**macOS 14 Sonoma and earlier:** Control-click (or right-click) the app in Applications, choose
**Open**, then **Open** in the dialog. The System Settings route above works there too.

After that, macOS remembers your decision for this copy of the app and starts it normally.

**"Vice Mail is damaged and can't be opened."** macOS says this when the app's signature does not
match its files, for example after a broken download. The build checks the signature with
`codesign --verify --deep --strict` before it is shipped, so first compare the checksum (see above)
and download the disk image again. If the checksum matches and the message stays, please report
it: that would be a build problem.

**About `xattr`:** you will find advice to run `xattr -d com.apple.quarantine` (or `xattr -cr`) on
an app that macOS blocks. That deletes the quarantine attribute, so Gatekeeper skips its check for
this copy entirely. It gets you past the same dialog as **Open Anyway**, but without macOS
recording that you allowed this app, and it also silences the "damaged" warning, which may be
pointing at a real problem. Prefer the System Settings route. If you remove the attribute anyway,
verify the checksum first and remove it only from this app, never from a whole folder.

**Is my copy signed?** In Terminal:

```sh
codesign --verify --deep --strict --verbose=2 "/Applications/Vice Mail.app"   # valid on disk (both kinds)
codesign -dvv "/Applications/Vice Mail.app" 2>&1 | grep -E '^(Signature|Authority|TeamIdentifier)='
spctl --assess -vv "/Applications/Vice Mail.app"
```

| | signed build | unsigned build |
|---|---|---|
| `codesign -dvv` | `Authority=Developer ID Application: …`, a `TeamIdentifier` of ten characters | `Signature=adhoc`, `TeamIdentifier=not set` |
| `spctl --assess -vv` | `accepted`, `source=Notarized Developer ID` | `rejected` (no Developer ID), as expected |
| `xcrun stapler validate "/Applications/Vice Mail.app"` (needs Xcode or its command line tools) | `The validate action worked!` | fails: no ticket |

### Updating

**In the app** (once the project is published on GitHub; see "Updates from the app" below):
**Settings → About → Check now**, then **Download update** and **Restart to install**. The app
replaces itself in Applications and opens again; your accounts and mail stay. This only works for
the copy in Applications: an app started from the disk image, or one macOS still runs from a
temporary place because it was never moved (App Translocation), cannot replace itself and says so.

**By hand:** quit the app, then drag the new version from its disk image onto Applications and
replace the old one. Your accounts and mail stay.

**Keychain question after an update:** macOS ties the passwords in your keychain to the app's
signature. Unsigned builds have no lasting signature identity, so after every update to an unsigned
version, and once more on the move to the first signed version, macOS asks once per stored entry
(one per account, plus the settings sync key and AI tokens if you use them) whether "Vice Mail" may
use it. Enter your Mac's password and choose **Always Allow**. **Allow** lets only this one access
through, and the app asks again at its next connection. Signed versions keep the same signature
identity, so later updates should not ask again.

Do not go back to an older version: once a newer version has upgraded the local database, an
older one refuses to open it.

## Windows

### Install

1. Your browser may warn that the file is not commonly downloaded. Choose to keep it (in Edge:
   **…** → **Keep**, then **Show more** → **Keep anyway**).
2. Run `Vice-Mail-<version>-windows-x64-setup.exe`.
3. **SmartScreen** shows "Windows protected your PC" (*Der Computer wurde durch Windows
   geschützt*) because the installer is not signed. Click **More info** (*Weitere Informationen*).
   Check that the app name is the installer's file name and the publisher reads *Unknown
   publisher*, then click **Run anyway** (*Trotzdem ausführen*).
4. **Smart App Control:** if it is turned on, Windows blocks unsigned programs outright, without a
   **Run anyway** button. There is no per-app exception. We do not recommend turning Smart App
   Control off for this app: the switch applies to every program on the PC, and depending on the
   Windows version it cannot be turned back on without resetting Windows. Wait for signed builds
   instead, or use a PC where it is off already.
5. The installer works per user: it needs no administrator rights, so there is no User Account
   Control prompt. It installs to `%LOCALAPPDATA%\Vice Mail`, adds a Start menu entry and, if you
   tick the box on the last page, a desktop shortcut. It speaks English or German, following
   Windows.
6. **WebView2:** Windows 11 includes the Microsoft Edge WebView2 Runtime. If it is missing, the
   installer downloads Microsoft's WebView2 bootstrapper and installs the runtime silently. That
   needs an internet connection and talks to Microsoft; the app itself never does.

### Updating

**In the app** (once the project is published on GitHub; see "Updates from the app" below):
**Settings → About → Check now**, then **Download update** and **Restart to install**. The app
closes, the installer runs in passive mode (a progress bar, no questions, no "Already Installed"
page) and starts the app again. Your accounts and mail stay.

**By hand:** run the installer of the new version (SmartScreen asks again, as on the first
install). Because a version is already installed, the installer shows the page **Already
Installed** (*Bereits installiert*): "An older version of Vice Mail is installed on your system.
It's recommended that you uninstall the current version before installing."

1. Leave the preselected option **Uninstall before installing** (*Vor der Installation
   deinstallieren*) and click **Next**. That does both in one go: the old version's uninstaller
   opens, and after it the installer continues with the new version.
2. In the uninstaller, leave **Delete the application data** (*Lösche die Anwendungsdaten*)
   **unticked** and click **Uninstall**. Unticked, your accounts, stored mail and settings in
   `%LOCALAPPDATA%\org.vicemail.desktop` stay, and the passwords in Credential Manager are never
   touched by the uninstaller anyway. Ticked, the app starts empty and you set up your accounts
   again.
3. Finish the installer as on the first install.

**Do not uninstall** (*Nicht deinstallieren*) also works: the new version is installed over the
old one, with the same data.

As on macOS, do not go back to an older version: the installer warns that a newer version is
installed, and an older app refuses a database a newer one has upgraded.

### Old icon after an update

Windows caches app icons. After an update that changes the icon, a pinned taskbar button, the
Start menu or a desktop shortcut can keep showing the old one. To refresh it, try these in order:

1. Press **Win+R**, run `ie4uinit.exe -show` and wait a few seconds.
2. Unpin Vice Mail (right-click its taskbar button → **Unpin from taskbar**, *Von Taskleiste
   lösen*), start the app, then pin it again (**Pin to taskbar**, *An Taskleiste anheften*).
3. Sign out of Windows and back in, or restart the PC.

## Updates from the app

The app can update itself, but it never checks on its own unless you allow it:

- **Settings → About** shows the installed version and the switch **Check for updates
  automatically**. It is **off** until you turn it on. After you set up your first account the app
  asks once whether to turn it on; "No, thanks" leaves it off.
- When it is on, the app asks `github.com`, where Vice Mail is published, at most once a day
  whether there is a newer version. GitHub sees your IP address and the time of the request, and,
  when you download an update, which file. The app sends no account data, no usage data and no
  identifier. **Check now** asks once, also while the switch is off.
- A newer version is only offered, never downloaded or installed by itself: **Download update**
  fetches it, and the app checks that it is signed with the project's update key and for exactly
  this version. A file that does not match is discarded ("The update could not be verified").
  **Restart to install** installs it and opens the app again.

## Where the app keeps its data

The folders are named after the app's identifier, `org.vicemail.desktop`.

| What | macOS | Windows |
|---|---|---|
| App | `/Applications/Vice Mail.app` | `%LOCALAPPDATA%\Vice Mail` |
| Database and stored mail | `~/Library/Application Support/org.vicemail.desktop` | `%LOCALAPPDATA%\org.vicemail.desktop` |
| Logs | `~/Library/Logs/org.vicemail.desktop` | `%LOCALAPPDATA%\org.vicemail.desktop\logs` |
| Caches (copies of opened attachments, web view cache) | `~/Library/Caches/org.vicemail.desktop` | `%LOCALAPPDATA%\org.vicemail.desktop\attachments`, web view cache under `EBWebView` |
| Web view data | `~/Library/WebKit/org.vicemail.desktop` | `%LOCALAPPDATA%\org.vicemail.desktop\EBWebView` |
| Passwords | login keychain: one *application password* per account, service `org.vicemail.desktop`, account name = the account's ID | Credential Manager → Windows Credentials → Generic Credentials: one entry `<account ID>.org.vicemail.desktop` per account |

Passwords are never written to the database, the logs or any file of the app; they live only in
the system's keychain or Credential Manager.

## Uninstall

Remove your accounts in the app **first**: **Settings → Accounts →** the account **→ Remove
account…**. That deletes the mail the app stored for the account and its password from the keychain
or Credential Manager. Your mail on the server is not touched. Uninstalling the app alone leaves the
passwords behind.

### macOS

1. Remove the accounts in the app (see above) and quit it.
2. Drag `/Applications/Vice Mail.app` to the Trash.
3. Delete the data folders from the table above. To find everything the app left in your Library:

   ```sh
   find ~/Library -maxdepth 2 -name 'org.vicemail.desktop*'
   ```

4. Passwords of accounts you did not remove in the app: open **Keychain Access**, search for
   `org.vicemail.desktop` and delete the entries. Or in Terminal, once per entry:

   ```sh
   security find-generic-password -s org.vicemail.desktop        # shows one entry and its "acct"
   security delete-generic-password -s org.vicemail.desktop -a <account ID>
   ```

### Windows

1. Remove the accounts in the app (see above) and close it.
2. **Settings → Apps → Installed apps →** Vice Mail **→ Uninstall**. Tick **Delete the
   application data** to remove `%LOCALAPPDATA%\org.vicemail.desktop` as well. No administrator
   rights are needed.
3. Passwords of accounts you did not remove in the app stay in Credential Manager; the uninstaller
   does not delete them. Open **Control Panel → Credential Manager → Windows Credentials** and
   remove the entries ending in `.org.vicemail.desktop`, or in a terminal:

   ```bat
   cmdkey /list | findstr vicemail
   cmdkey /delete:<account ID>.org.vicemail.desktop
   ```

## Status of this guide

Checked on the build Mac (macOS 27.2, Apple M4): the disk image mounts, contains the app and the
Applications link, the app's ad-hoc signature verifies, Gatekeeper's assessment is `rejected` as
described, and the app starts from the disk image. **Signed builds** are prepared but none exists
yet (the Apple developer membership is not active): the signing path was checked with a
self-signed test certificate only (hardened runtime, the app starts from the disk image). The
"signed build" column above and the single **Open** prompt are what Apple documents; they will be
confirmed with the first notarised build on a second Mac. **Not yet checked:** the Gatekeeper dialogs on a
second Mac, everything on Windows (SmartScreen, Smart App Control, installer, WebView2 download,
uninstaller). The Windows steps follow the installer's configuration and the manual test plan; they
will be confirmed or corrected after the first test on a Windows PC.

**In-app updates** are tested against a local update server only (signature check, a manipulated
file refused, no connection without consent); the first real update from GitHub can only be tried
once a public release exists. The two update paths on Windows above follow the installer's
configuration and have not been seen on a Windows PC yet.
