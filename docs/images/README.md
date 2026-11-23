# Screenshots

Images used by the top-level `README.md`. None exist yet; the list below says which views to
capture. Once a file is here, uncomment its line in the "Screenshots" section of `README.md`.

## Rules

- **Only test data.** Capture with the local test server, never with a real mailbox:
  `scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000`, then
  add `alice@vicemail.test` (password `alice-test-pw`, see `testmail/README.md`) and, for the
  unified inbox, `bob@vicemail.test`. Nothing personal may be visible: no real names, addresses,
  folders, notifications or other windows.
- Default settings unless the view needs otherwise: orange accent, comfortable density, text size
  100 %, English interface.
- Window about 1440 × 900 points on a Retina display (PNG at 2×). Crop to the window with its
  shadow (`Cmd+Shift+4`, then Space, then click the window). Keep each file under about 500 KB
  (`pngquant` or `oxipng` if needed).
- File names as below, lower case.

## Views

| File | View | Notes |
|---|---|---|
| `inbox-light.png` | Unified inbox with the reading pane on the right, light mode | Conversations on, a newsletter open so the bar "remote images blocked" shows |
| `inbox-dark.png` | The same in dark mode | Same message as above |
| `reading-links.png` | A message with the link check open | Click a link whose text and target differ, so the warning shows |
| `composer.png` | Composer window with formatting, a signature and an attachment | Reply to a seed message |
| `search.png` | Search with operators and highlighted results | For example `from:bob has:attachment` |
| `command-palette.png` | Command palette (`Cmd+K`) over the inbox | Type a few letters so the list is filtered |
| `ai-consent.png` | AI in the app: the step that shows what would be sent to a cloud provider | Needs an AI profile; a local Ollama profile is enough to show the panel, the consent step needs a cloud profile (no request has to be sent) |
| `mcp.png` | Settings → AI over MCP: access per account and the list of AI clients | No client needs to be connected |
| `windows-inbox.png` | Unified inbox on Windows 11 | Once the Windows build has been tested on a Windows PC |

The desktop tests (`pnpm --dir app test:e2e`) also write screenshots to
`app/tests/e2e/screenshots/`. They come from a test build and are not meant for the README.
