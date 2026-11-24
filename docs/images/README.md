# Screenshots

The top-level `README.md` shows `inbox-light.png`; the other views are kept for documents and
release notes. Every view in the table below exists except `windows-inbox.png`, which needs a
Windows PC. A desktop spec takes them, so they can be taken
again after the interface changes.

## Taking them again

```
pnpm --dir app build:e2e
README_SHOTS=1 scripts/e2e-gui.sh --spec ./tests/e2e/readme-shots.e2e.mjs
```

`app/tests/e2e/readme-shots.e2e.mjs` resets the local test server, seeds it, adds alice and bob,
sets each view up and writes the PNGs into this folder. It is not part of `pnpm test:e2e`:
`app/wdio.conf.mjs` excludes it unless `README_SHOTS=1` is set. Like every desktop spec it needs a
graphical login and takes the test-server lock; on macOS `scripts/e2e-gui.sh` runs it in the GUI
session (see the script). A run takes about a minute. Check the pictures before committing
them: the spec checks sizes, scale and that no path or user name is visible, not how they look.

How the pictures differ from a screenshot taken by hand:

- **Web content only.** WebDriver renders the page, not the window: no window frame, no rounded
  corners, no shadow. The three window buttons are drawn into the page where macOS puts them
  (`e2e_window_controls`), so the main window looks as it does on screen. `screencapture -l`
  would include frame and shadow, but it needs the Screen Recording permission, which the
  launchd job of `e2e-gui.sh` does not have. The composer is its own window; its picture is the
  page without the title bar.
- **2× without a Retina screen.** If the screen is 1× (a Mac mini without a display has a 1920 ×
  1080 fallback screen), the spec adds a virtual HiDPI display for the run
  (`app/tests/e2e/support/hidpi-display.m`, CoreGraphics' private `CGVirtualDisplay`, built with
  the system's clang) and moves the window there. It keeps the screen awake meanwhile
  (`caffeinate`), because macOS removes a virtual display only while the screen is awake. Should
  a run end without removing it, `caffeinate -u -t 2` in the GUI session wakes the screen and
  the display goes.
- **Size.** Pictures over 500 KB are re-encoded by `app/tests/e2e/support/png.mjs` (lossless,
  or with a 256-colour palette in which the most frequent colours stay exact; mean error below
  0.05 of 255). No extra tool or package is needed.
- **No hover.** The pointer is moved to a corner of the virtual display. The link check comes
  last because its real click leaves WebKit's pointer over the window.

## Rules

- **Only test data.** Capture with the local test server, never with a real mailbox:
  `scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000 --no-smtp`,
  then add `alice@vicemail.test` (password `alice-test-pw`, see `testmail/README.md`) and, for the
  unified inbox, `bob@vicemail.test`. `--no-smtp` leaves out the seven SMTP transport tests,
  whose subjects ("SMTP test 1: submission with STARTTLS") would otherwise top the inbox. Nothing
  personal may be visible: no real names, addresses, folders, notifications, other windows, home
  or temp paths (Settings → AI over MCP shows the helper's path above the shared accounts: keep
  it out of the picture).
- Default settings unless the view needs otherwise: orange accent, comfortable density, text size
  100 %, English interface, accounts read-only as after setup.
- Window about 1440 × 900 points on a Retina display (PNG at 2×). Keep each file under about
  500 KB.
- File names as below, lower case.

## Views

| File | View | Notes |
|---|---|---|
| `inbox-light.png` | Unified inbox with the reading pane on the right, light mode | Conversations on, a newsletter open so the bar "remote images blocked" shows |
| `inbox-dark.png` | The same in dark mode | Same message as above |
| `reading-links.png` | A message with the link check open | Click a link whose text and target differ, so the warning shows (the seed's `html/hostile` message) |
| `composer.png` | Composer window with formatting, a signature and an attachment | A forward of a seed message, which keeps its attachment: an attachment of one's own needs a file dropped on the window, which WebDriver cannot do. Window 820 × 900 points, so the signature shows |
| `search.png` | Search with operators and highlighted results | `has:attachment report` |
| `command-palette.png` | Command palette (`Cmd+K`) over the inbox | Type a few letters so the list is filtered (`go`) |
| `ai-consent.png` | AI in the app: the step that shows what would be sent to a cloud provider | A cloud profile (the Anthropic preset, no key) with the account allowed for cloud providers; nothing is sent |
| `mcp.png` | Settings → AI over MCP: access per account and the list of AI clients | One client added, none connected |
| `windows-inbox.png` | Unified inbox on Windows 11 | Once the Windows build has been tested on a Windows PC |

The other desktop tests (`pnpm --dir app test:e2e`) also write screenshots to
`app/tests/e2e/screenshots/`. They come from a test build and are not meant for the README.
