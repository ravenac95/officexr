#!/usr/bin/env bash
# Builds libbevy_flutter_host as a static library for iOS devices and the arm64 simulator,
# and packages both with the C header into the bevy_host plugin's BevyHostCore.xcframework.
# Runs on macOS with Xcode and the Rust targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
out="$here/bevy_host/ios/bevy_host/BevyHostCore.xcframework"
# Match the plugin's and the Flutter app's deployment target.
export IPHONEOS_DEPLOYMENT_TARGET=15.0

cd "$here/native"
libs=()
for target in aarch64-apple-ios aarch64-apple-ios-sim; do
  cargo rustc --lib --release --target "$target" --crate-type staticlib \
    -- --print native-static-libs
  libs+=(-library "target/$target/release/libbevy_flutter_host.a" -headers "$here/bevy_host/ios/include")
done

rm -rf "$out"
xcodebuild -create-xcframework "${libs[@]}" -output "$out"
