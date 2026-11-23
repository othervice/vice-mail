#!/bin/bash
# Runs the desktop tests (pnpm --dir app test:e2e, or test:e2e:run with arguments) as a launchd job
# in the logged-in user's GUI session, and waits for it.
#
# Why: the app keeps passwords in the login keychain. A shell that was started over SSH, or by a
# launch agent before anyone logged in at the screen (tmux sessions after a reboot), runs in a
# security session of its own, where the login keychain stays locked even while the user is
# logged in graphically: every `add_account` then fails with "User interaction is not allowed".
# A launchd job in the domain gui/<uid> runs in the GUI session, where the keychain is unlocked.
# Needs a graphical login (macOS only).
#
#   scripts/e2e-gui.sh                      # build the test app and run every spec (test:e2e)
#   scripts/e2e-gui.sh --spec ./tests/e2e/frame.e2e.mjs   # no build, the given specs (test:e2e:run)
#   README_SHOTS=1 scripts/e2e-gui.sh --spec ./tests/e2e/readme-shots.e2e.mjs   # README screenshots
#
# The output goes to the terminal and to $E2E_LOG (default: a file in $TMPDIR). The exit code is
# the test run's.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
uid="$(id -u)"
if ! launchctl print "gui/$uid" >/dev/null 2>&1; then
  echo "e2e-gui: no graphical login for user $uid; log in at the screen first" >&2
  exit 2
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/vice-e2e-gui.XXXXXX")"
label="org.vicemail.e2e-gui.$$"
log="${E2E_LOG:-$work/e2e.log}"
status="$work/status"
plist="$work/job.plist"

if [ "$#" -eq 0 ]; then
  command=(pnpm --dir app test:e2e)
else
  command=(pnpm --dir app test:e2e:run "$@")
fi

# The job starts with launchd's bare environment: set what the run needs, then the test run.
runner="$work/run.sh"
{
  echo '#!/bin/bash'
  echo "cd $(printf '%q' "$ROOT")"
  echo 'export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"'
  echo '[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"'
  echo 'export LANG=en_US.UTF-8 CARGO_INCREMENTAL=0'
  for name in TESTMAIL_LOCK_NAME E2E_APP MAIL_APP_E2E_DATA_DIR README_SHOTS; do
    if [ -n "${!name:-}" ]; then echo "export $name=$(printf '%q' "${!name}")"; fi
  done
  printf '%q ' "${command[@]}"
  echo
  echo "echo \$? > $(printf '%q' "$status")"
} >"$runner"
chmod +x "$runner"

cat >"$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key><array><string>/bin/bash</string><string>$runner</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>StandardOutPath</key><string>$log</string>
  <key>StandardErrorPath</key><string>$log</string>
</dict></plist>
EOF

: >"$log"
cleanup() {
  launchctl bootout "gui/$uid/$label" >/dev/null 2>&1 || true
  [ -n "${tail_pid:-}" ] && kill "$tail_pid" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

echo "e2e-gui: running '${command[*]}' in gui/$uid; log: $log"
launchctl bootstrap "gui/$uid" "$plist"
tail -n +1 -f "$log" &
tail_pid=$!
idle=0
while [ ! -s "$status" ]; do
  sleep 2
  # Ended without writing its status (killed, or the shell failed): stop waiting.
  if launchctl print "gui/$uid/$label" 2>/dev/null | grep -q "state = not running"; then
    idle=$((idle + 1))
    if [ "$idle" -ge 3 ] && [ ! -s "$status" ]; then
      echo "e2e-gui: the job ended without a result" >&2
      exit 3
    fi
  fi
done
sleep 1
code="$(cat "$status")"
echo "e2e-gui: finished with exit code $code"
exit "$code"
