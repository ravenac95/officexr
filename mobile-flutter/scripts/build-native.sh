#!/usr/bin/env bash
# Builds libbevy_flutter_host.so for each Android ABI into the Flutter app's jniLibs.
# Needs ANDROID_NDK_HOME, cargo-ndk, and the Rust targets for the ABIs:
#   rustup target add aarch64-linux-android x86_64-linux-android
# Usage: build-native.sh [abi ...]   (default: arm64-v8a x86_64)
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
abis=("$@")
if [ ${#abis[@]} -eq 0 ]; then
  abis=(arm64-v8a x86_64)
fi

targets=()
for abi in "${abis[@]}"; do
  targets+=(-t "$abi")
done

cd "$here/native"
cargo ndk "${targets[@]}" -P 26 -o "$here/app/android/app/src/main/jniLibs" build --release
