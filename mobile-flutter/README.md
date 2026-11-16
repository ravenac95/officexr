# Bevy + Flutter prototype

Does a Flutter app work as the shell for OfficeXR's Bevy renderer? This prototype says yes on
Android and iOS: Flutter owns the app and its UI, and a Bevy scene renders behind Flutter
widgets. On Android, Bevy draws into a Flutter texture. On iOS, it draws into a Metal view
embedded as a platform view.

- `native/`: the Rust library `libbevy_flutter_host.so`.
  - `host.rs` runs a Bevy app on a surface someone else owns, without `bevy_winit`: it spawns the
    primary window entity with the surface's `RawHandleWrapper` before `RenderPlugin` builds, then
    drives `app.update()` itself.
  - It shows a `SceneSpec` Flutter picks:
    - a live office: `OfficeRoomPlugin` from `crates/officexr-client`, built with
      `default-features = false` so its standalone winit app stays out; `office.rs` reports
      joined, peers and disconnects back to Flutter;
    - or the demo (`scene.rs`), spinning cubes that need no server. CI's smoke tests run it.
  - `android.rs` is the JNI glue: Bevy runs on a thread of its own.
  - `ios.rs` is a C API. wgpu only creates Metal surfaces on the main thread, so Swift steps the
    host from a `CADisplayLink` instead.
- `bevy_host/`: the iOS plugin (a Swift package). `BevyMetalView` is a `CAMetalLayer`-backed
  `UIView` that Bevy renders into and that sends touches straight to Rust. It links the Rust
  library as `BevyHostCore.xcframework`, built by `scripts/build-ios-native.sh` (gitignored).
- `app/`: the Flutter app.
  - `BevyTexture.kt` creates a `SurfaceProducer` (a Flutter texture backed by an Android
    `Surface`) and hands its `Surface` to Rust.
  - `bevy_view.dart` shows the texture and forwards pointer events.
  - `join_screen.dart` asks for the server, realtime URL, office id and session cookie,
    remembered with `flutter_secure_storage`. It also opens the demo.
  - `office_screen.dart` shows the room with a status bar. Bevy draws the walk stick and
    handles look-drag itself.
  - `demo_screen.dart` lays Flutter controls (speed, colour, cube count, a second Flutter page)
    over the demo.

Android:

```
Dart widgets ──MethodChannel──▶ Kotlin BevyTexture ──JNI──▶ Rust host thread ──▶ Bevy app
      ▲                                │ Surface                                    │ renders to
      └────────EventChannel◀── Kotlin listener ◀──JNI── event thread ◀── HostEvent ──┘ the Surface
Texture(textureId) ◀── Flutter composites the SurfaceProducer's frames
```

iOS:

```
Dart widgets ──MethodChannel──▶ BevyHostPlugin ──C──▶ Host (main thread) ──▶ Bevy app
UiKitView ◀── BevyMetalView (CAMetalLayer) ◀── renders; CADisplayLink calls bevy_host_frame
      touches ──────────────────────▶ bevy_host_touch
```

## Get the app

CI (`.github/workflows/mobile-flutter.yml`) builds on every push that touches this folder and
publishes the rolling **`bevy-flutter-prototype`** pre-release under the repo's Releases. The
Android jobs run on Linux and the iOS job on macOS; both are free on a public repo.

- **Android:** install `app-arm64-v8a-release.apk`; you need to allow installs from unknown
  sources.
- **iOS:** `bevy-flutter-ios-unsigned.ipa` is unsigned, so sideload it:
  1. Install [Sideloadly](https://sideloadly.io) on a Mac (or Windows PC).
  2. Plug in the iPhone, drop the `.ipa` into Sideloadly, sign in with your Apple ID, and press
     Start.
  3. On the iPhone, trust your Apple ID under *Settings → General → VPN & Device Management*.
     On iOS 16 and later, also turn on *Settings → Privacy & Security → Developer Mode*.

  With a free Apple ID the install expires after 7 days; sideload the latest build again.

The workflow artifacts also keep the builds and the emulator/simulator smoke tests'
screenshots.

## Join an office

1. Sign in to OfficeXR on the web and open the office. Its id is in the URL.
2. In your browser's dev tools (Network tab), copy the `Cookie` request header of any
   `/api/me` request.
3. In the app, enter:
   - **Server URL:** the API origin, the web server's `PUBLIC_URL`.
   - **Realtime URL:** its WebSocket, usually the same host with `wss://` and `/ws`.
   - **Office id** and **session cookie** from steps 1 and 2.
4. Tap **Join office**. Walk with the stick bottom-left, and drag anywhere else to look.

The app joins as `<your login>#native`, so the same account can stay signed in on the web at the
same time (if the office allows several sessions per login). The cookie expires with your web
session; paste a fresh one if joining fails with a 401.

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
Rebuild them whenever `native/` changes; `flutter run` doesn't. On a Mac, iOS is the same with
`scripts/build-ios-native.sh` (it needs `rustup target add aarch64-apple-ios
aarch64-apple-ios-sim`), then `flutter run` on a simulator or device.

## What it doesn't do yet

- **iOS on a real phone, untested.** CI only proves the simulator; real-device runs depend on
  sideloading. The simulator slice is arm64 only (Apple-silicon Macs).
- **Sign-in.** A session cookie pasted from the browser, not a native Google sign-in.
- **The office is the native client's.** Walking, peers, avatars and the world are there; voice,
  chat and the web's HUD are not.
- **Connecting blocks.** `Host::new` waits on the `/api/me` login request before drawing. On
  iOS that's the main thread.
- **Size.** The Rust library is about 64 MB per ABI with the office renderer and networking;
  the arm64 APK is about 80 MB.
- **Lifecycle polish.** On Android, resizing or regaining the surface restarts Bevy, so scene
  state resets, and frames are paced by a fixed 60 fps sleep, not vsync. On iOS, Bevy runs on
  the main thread, so a slow frame stalls platform-channel calls (Flutter's own UI keeps
  going).
- **One platform layer each.** Android's bridge lives in the app (`BevyTexture.kt`) and iOS's in
  the `bevy_host` plugin; moving Android into the plugin would make them symmetric.
- **Input.** On Android, touches go through a method channel (one async hop per pointer
  event); `dart:ffi` straight into the library would cut that. On iOS, the native view hands
  them to Rust directly.
