# Bevy + Flutter prototype

Does a Flutter app work as the shell for OfficeXR's Bevy renderer? This prototype says yes on
Android: Flutter owns the app and its UI, and a Bevy scene renders into a Flutter texture behind
Flutter widgets.

- `native/`: the Rust library `libbevy_flutter_host.so`.
  - `host.rs` runs a Bevy app on a surface someone else owns, without `bevy_winit`: it spawns the
    primary window entity with the surface's `RawHandleWrapper` before `RenderPlugin` builds, then
    drives `app.update()` itself.
  - `scene.rs` is the demo: spinning cubes and an orbit camera, with commands in and events out.
  - `android.rs` is the JNI glue.
- `app/`: the Flutter app.
  - `BevyTexture.kt` creates a `SurfaceProducer` (a Flutter texture backed by an Android
    `Surface`) and hands its `Surface` to Rust.
  - `bevy_view.dart` shows the texture and forwards pointer events.
  - `main.dart` lays Flutter controls (speed, colour, cube count, a second Flutter page) over it.

```
Dart widgets ──MethodChannel──▶ Kotlin BevyTexture ──JNI──▶ Rust host thread ──▶ Bevy app
      ▲                                │ Surface                                    │ renders to
      └────────EventChannel◀── Kotlin listener ◀──JNI── event thread ◀── HostEvent ──┘ the Surface
Texture(textureId) ◀── Flutter composites the SurfaceProducer's frames
```

## Get the APK

CI (`.github/workflows/mobile-flutter.yml`) builds on every push that touches this folder and
publishes the rolling **`bevy-flutter-prototype`** pre-release under the repo's Releases.
Install `app-arm64-v8a-release.apk` on a phone; you need to allow installs from unknown sources.
The same APKs and the emulator smoke test's screenshots are also kept as workflow artifacts.

## Build locally

Needs Flutter 3.50, the Android SDK with NDK 28.2.13676358, `cargo-ndk`, and the Rust targets:

```bash
rustup target add aarch64-linux-android x86_64-linux-android
```

```bash
ANDROID_NDK_HOME=$ANDROID_HOME/ndk/28.2.13676358 mobile-flutter/scripts/build-native.sh
```

```bash
cd mobile-flutter/app && flutter run --release
```

`build-native.sh` writes the `.so` files into `app/android/app/src/main/jniLibs/` (gitignored).
Rebuild them whenever `native/` changes; `flutter run` doesn't.

## What it doesn't do yet

- **iOS.** The same `host.rs` should work behind a `CAMetalLayer` (a Flutter texture through
  `FlutterTexture`/`CVPixelBuffer`, or a platform view), but it needs macOS runners.
- **OfficeXR itself.** The scene is a stand-in. Next steps: draw a room with
  `crates/officexr-render`, and connect with `crates/officexr-protocol`.
- **Lifecycle polish.** Resizing or regaining the surface restarts Bevy, so scene state resets.
  Frames are paced by a fixed 60 fps sleep, not the display's vsync.
- **Input.** Touches go through a method channel (one async hop per pointer event); `dart:ffi`
  straight into the library would cut that.
