#!/usr/bin/env bash
# Installs the APK on the running emulator, launches it, and checks Bevy came up inside Flutter:
# the host's `ready` and `stats` events reach Kotlin (logged under the BevyFlutter tag) and no
# `error` event or crash shows up. Leaves screenshots and logcat in the output directory.
# Usage: emulator-smoke.sh <apk> <out-dir>
set -euo pipefail

apk="$1"
out="$2"
package=dev.officexr.bevy_flutter
mkdir -p "$out"

adb install -r "$apk"
adb logcat -c
adb shell am start -W -n "$package/.MainActivity"

ready=""
for _ in $(seq 1 90); do
  if adb logcat -d -s BevyFlutter:I | grep -q '"type":"ready"'; then
    ready=1
    break
  fi
  sleep 2
done

# Let a few stats events through, then exercise the touch path with a one-finger orbit.
sleep 10
adb exec-out screencap -p > "$out/1-launched.png"
read -r width height < <(adb shell wm size | sed -n 's/.*: \([0-9]*\)x\([0-9]*\).*/\1 \2/p' | tail -1)
adb shell input swipe $((width / 5)) $((height / 2)) $((width * 4 / 5)) $((height / 2)) 800
sleep 3
adb exec-out screencap -p > "$out/2-after-orbit.png"
adb logcat -d > "$out/logcat.txt"
adb logcat -d -s BevyFlutter:I > "$out/bevy-events.txt" || true

fail() {
  echo "smoke test failed: $1" >&2
  tail -n 80 "$out/logcat.txt" >&2
  exit 1
}

[ -n "$ready" ] || fail "Bevy never reported ready"
grep -h '"type":"ready"' "$out/bevy-events.txt" | head -1
if grep -q '"type":"error"' "$out/bevy-events.txt"; then
  fail "Bevy reported an error: $(grep -h '"type":"error"' "$out/bevy-events.txt" | head -1)"
fi
adb shell pidof "$package" > /dev/null || fail "the app is no longer running"
grep -q '"type":"stats"' "$out/bevy-events.txt" || fail "no frame stats after ready"
grep -h '"type":"stats"' "$out/bevy-events.txt" | tail -1
echo "smoke test passed"
