#!/usr/bin/env bash
# Installs a simulator build of the app on a fresh iPhone simulator, launches it into the demo
# scene, and checks Bevy came up inside Flutter: the host's `ready` and `stats` events reach
# Swift (printed with a BevyFlutter prefix) and no `error` event shows up. Leaves screenshots
# and the app's console output in the output directory.
# Usage: ios-simulator-smoke.sh <Runner.app> <out-dir>
set -euo pipefail

app="$1"
out="$2"
bundle=dev.officexr.bevyFlutter
mkdir -p "$out"

# The newest available iPhone runtime's first iPhone.
udid="$(xcrun simctl list devices available --json | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
for runtime in sorted(devices, reverse=True):
    if "iOS" not in runtime:
        continue
    for device in devices[runtime]:
        if device["name"].startswith("iPhone"):
            print(device["udid"])
            sys.exit(0)
sys.exit("no available iPhone simulator")
')"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl install "$udid" "$app"

console="$out/console.txt"
xcrun simctl launch --console-pty "$udid" "$bundle" demo > "$console" 2>&1 &
launcher=$!

ready=""
for _ in $(seq 1 90); do
  if grep -q 'BevyFlutter {"type":"ready"' "$console"; then
    ready=1
    break
  fi
  sleep 2
done
sleep 10
xcrun simctl io "$udid" screenshot "$out/1-launched.png"
sleep 3
xcrun simctl io "$udid" screenshot "$out/2-later.png"
kill "$launcher" 2>/dev/null || true
grep 'BevyFlutter' "$console" > "$out/bevy-events.txt" || true

fail() {
  echo "smoke test failed: $1" >&2
  tail -n 80 "$console" >&2
  exit 1
}

[ -n "$ready" ] || fail "Bevy never reported ready"
grep -h '"type":"ready"' "$out/bevy-events.txt" | head -1
if grep -q '"type":"error"' "$out/bevy-events.txt"; then
  fail "Bevy reported an error: $(grep -h '"type":"error"' "$out/bevy-events.txt" | head -1)"
fi
grep -q '"type":"stats"' "$out/bevy-events.txt" || fail "no frame stats after ready"
grep -h '"type":"stats"' "$out/bevy-events.txt" | tail -1
echo "smoke test passed"
