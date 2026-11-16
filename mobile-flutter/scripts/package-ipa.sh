#!/usr/bin/env bash
# Wraps an unsigned Runner.app (from `flutter build ios --release --no-codesign`) into an .ipa
# for sideloading tools such as Sideloadly, which sign it with the user's Apple ID.
# Usage: package-ipa.sh <Runner.app> <out.ipa>
set -euo pipefail

app="$1"
ipa="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
work="$(mktemp -d)"
mkdir "$work/Payload"
cp -R "$app" "$work/Payload/"
rm -f "$ipa"
(cd "$work" && zip -qry "$ipa" Payload)
rm -rf "$work"
ls -lh "$ipa"
