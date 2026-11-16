# Realtime media on mobile (Flutter + Bevy + Jitsi): exploration

Status: research and design only, no implementation. Facts checked on 2026-11-16 against the
sources at the end. Anything marked **UNVERIFIED** could not be confirmed from a primary source
and needs a spike before we rely on it.

## TL;DR: recommendation

1. **Port the web design, not a new one.** Web proximity voice does not adjust volume by distance. It
   switches between whole Jitsi conferences: one per proximity group and one per voice zone, in
   a hidden Jitsi view driven by commands. Mobile can match that exactly without any
   per-participant audio control.
2. **Phase 1 should embed the native Jitsi Meet SDK's `JitsiMeetView` as a Flutter platform
   view.**
   - Use our own thin plugin, or a fork of the Apache-2.0 `jitsi_meet_flutter_sdk`. Keep the view
     hidden for voice, and show it as a panel while someone shares a screen.
   - This is the mobile equivalent of the web's hidden `<JaaSMeeting>` iframe. It works with our
     existing server-minted JaaS JWT. The SDK already handles audio sessions, CallKit and
     ConnectionService, background audio and screen share.
   - The stock `jitsi_meet_flutter_sdk.join()` can't be used as is: it opens a full-screen Jitsi
     activity or view controller over our app.
3. **The proximity decision lives in Rust; the media lives in Dart and native code.**
   - The Bevy host already runs the world and can run `officexr-sim`'s `PairTracker`, the Rust
     port of the SDK's proximity tracker.
   - It sends a "voice room changed" event over the host event channel that already exists.
   - A Dart `VoiceController` hashes the room name, fetches the JWT, and drives the Jitsi view.
4. **Defer in-world video (camera on avatars, screen share on a wall) and gain-based proximity.**
   Both need decoded WebRTC frames and per-track audio, which the Jitsi Meet SDK never exposes.
   They need a different media stack: our own WebRTC client (flutter_webrtc or a Rust WebRTC
   crate) talking Jitsi's XMPP/Jingle protocol, or a move off Jitsi to a toolkit-style SFU such
   as LiveKit. Revisit when there is a product need. Web has no camera video today either.
5. **iOS screen *sending* is the riskiest feature for our sideload distribution.**
   - Through iOS 26 it needs a ReplayKit broadcast extension plus an App Group. A free Apple ID
     *can* have App Groups, but each extension uses one of the 10 weekly App IDs, and re-signing
     tools rewrite group IDs.
   - iOS 27 deprecates that path in favour of ScreenCaptureKit, with no extension needed.
   - Ship viewing first. Ship sending on Android first, then iOS once a spike shows it survives
     Sideloadly.

## 1. How web voice works today (the parity target)

From the code on this branch:

- **Who decides the room.** `packages/core-refactor/src/communication/communication.ts`, the
  `Communication` class, listens to the SDK bus.
  - `proximity:entered` (a peer crossed the inner sensor and the pair formed a MeetingArea) adds
    the peer to `nearby`; `proximity:exited` (the peer cleared the *outer* ring) removes it.
  - The `entering` and `exiting` bands are visual only.
  - The proximity itself is computed **client side** in the SDK. `tickProximity` and `PairTracker`
    are in `packages/sdk/src/spatial/`. The sensor radius is 3 m, the outer radius 6 m, and the
    enter debounce 500 ms (`world-constants.ts`). The realtime server only relays positions; it
    computes no proximity.
- **Room naming** (`jitsi-room.ts`):
  - Proximity room: `room-<officeId>-<lexmin(self ∪ nearby)>`. Everyone who agrees on the
    membership derives the same name.
  - Zone room: `zone-<officeId>-<zoneId>`.
  - Standing in a voice zone takes priority: you join after a 500 ms dwell, the zone call starts
    muted, and desk zones need two people standing still (`zone-presence.ts`).
  - Leaving is debounced by 1.5 s.
- **Room to Jitsi conference.** `JaasVoiceAdapter.jitsiRoomName` converts the room id to
  `officexr-` + the first 32 hex characters of the room id's SHA-256, because room names become
  XMPP JIDs.
  - Switching rooms disposes the iframe and mounts a new one (`JaasMeeting.tsx`, via
    `@jitsi/react-sdk` `JaaSMeeting`).
  - There is no per-participant gain and no Web Audio anywhere: **proximity is binary, by
    conference membership.**
- **Iframe config** (`JaasMeeting.tsx`):
  - Hidden: opacity 0, kept in the viewport so Chrome doesn't throttle it.
  - `startWithVideoMuted: true`, `prejoinPageEnabled: false`, `TOOLBAR_BUTTONS: []`.
  - Mute is driven with `toggleAudio`, and reconciled against `audioMuteStatusChanged`.
- **JWT** (`packages/server/src/jaasToken.ts`). The client calls `GET /api/offices/:id/voice-token`,
  authenticated by the session cookie; the web client is `packages/web/src/voice/jaasToken.ts`.
  - RS256, `iss: chat`, `aud: jitsi`, `sub: <appId>`, a 1 h TTL, refreshed 5 min before expiry.
  - `room: '*'`: JaaS can't scope a token to a room prefix, and we use many rooms.
  - `context.user`: name, an opaque id (sha256 of the login), and `moderator: true` for everyone.
  - Features: livestreaming, recording, transcription and outbound-call are all `'false'`. Screen
    share isn't a JWT feature (8x8 documents no `screen-sharing` key), so it's allowed.
- **Roster.** `CallRosterSync` mirrors Jitsi join and leave events into `callRoster`.
  - Each player broadcasts `jitsiRoom` and `jitsiParticipantId` in their status
    (`schemas/protocol.schema.json`). `CallList.tsx` uses them to map Jitsi participants to
    players.
  - The mobile Rust net client must send the same fields, or web users won't see mobile users in
    their call list.
- **Video and screen share on web:**
  - **Camera video doesn't exist**: calls start video-muted and there is no camera toggle.
  - **Screen share does**, inside the same conference: `toggleShareScreen`. When anyone shares,
    the hidden iframe is shown as a HUD panel, pinned to the sharer's `desktop` source, and it can
    be enlarged.
  - The older full-mesh WebRTC screen share described in `ARCHITECTURE.md` O10 is historical.

**Takeaway:** mobile parity needs four things:
- join and leave a named JaaS conference within about a second;
- mute and unmute;
- get participant join and leave events with ids;
- start a screen share, and show a remote share in a panel.

It does **not** need per-participant volume.

## 2. Jitsi integration options

### Comparison

| | A. `jitsi_meet_flutter_sdk` as shipped | **B. Native `JitsiMeetView` as a platform view** (fork or own plugin) | C. lib-jitsi-meet in a hidden WebView | D. flutter_webrtc + own XMPP/Jingle client | E. (not Jitsi) LiveKit `livekit_client` |
|---|---|---|---|---|---|
| Version / activity | 13.3.0, 2026-11-02, wraps iOS SDK 13.3.1 and Android 13.3.0. Released every 1–2 months | Same native SDKs | Very active (commits 2026-11-12), no GitHub releases | flutter_webrtc 1.6.3 (about 3 weeks old). **No Dart Jingle stack exists** | 2.13.2 (about 3 weeks old) |
| Licence | Apache-2.0 | Apache-2.0 | Apache-2.0 | MIT plus our own code | Apache-2.0 |
| Min OS | Android API 24, iOS 15.1 | Same (the RN SDK needs minSdk 26) | WKWebView getUserMedia needs iOS 14.3+ | — | — |
| Our own UI over the 3D view | **No.** `join()` launches `WrapperJitsiMeetActivity` on Android and presents `JitsiMeetViewController` `.overFullScreen` on iOS. `enterPiP()` leaves a floating tile | **Likely.** The view can be embedded on both platforms. Hidden or 1×1 with audio still running is **UNVERIFIED** (spike 1) | Yes (headless JS) | Yes | Yes (no UI shipped) |
| JaaS JWT | Yes (`token`) | Yes (`setToken`; the handbook names JaaS) | Yes (`new JitsiConnection(null, jwt, …)`, official `ljm-jaas-example`) | Yes in principle. JaaS support for fully custom clients is **UNVERIFIED** | n/a (LiveKit tokens) |
| Events we need | participantJoined/Left with id, screenShareToggled (remote too), audioMutedChanged | Same | Full lib API | Whatever we build | Full API |
| Per-participant volume | No API | No API | `volume` on audio elements is **read-only on iOS**, so we'd need a Web Audio GainNode per track | `Helper.setVolume(v, track)` | Via flutter_webrtc underneath (**UNVERIFIED** in the LiveKit API) |
| Screen share: Android | Yes (`android.screensharing.enabled`) | Yes; the SDK owns MediaProjection | **No**: no getDisplayMedia in Chrome Android; WebView **UNVERIFIED** | Yes (MediaProjection) | Yes |
| Screen share: iOS | Yes, with a broadcast extension + App Group + `RTCAppGroupIdentifier` / `RTCScreenSharingExtension` | Same | **Treat as no** (WKWebView getDisplayMedia unconfirmed) | Yes (broadcast extension, LiveKit's SampleHandler) | Yes (same) |
| Background audio | Yes (needs `audio` + `voip` background modes; call-integration flag) | Yes, same | **Weak.** WKWebView media stops in the background without native help (**UNVERIFIED** detail) | Yes, with native audio session work | Yes |
| Remote video frames for Bevy | No | No (rendered inside the React Native view) | No (inside the WebView) | **Yes** (native VideoSink) | Yes |
| App size | Large (React Native + WebRTC + Jitsi JS bundle). No official number: **UNVERIFIED, measure in spike 1** | Same | Small native cost; JS bundle about a few MB (**UNVERIFIED**) | WebRTC binary (several MB per ABI, **UNVERIFIED**) | Similar to D |
| Complexity | Lowest, but unusable | Low–medium: a plugin of about 2 platform views + a method channel; Android needs `JitsiMeetActivityDelegate` lifecycle forwarding | Medium: a JS app, a JS↔Dart bridge, native screen-share gap | **Very high**: XMPP over WebSocket, MUC, Jingle with jicofo, source-name signalling, bridge-channel constraints, SSRC rewriting; we would chase lib-jitsi-meet's protocol forever | Medium, but means a new backend (LiveKit Cloud or self-hosted) and a web migration |

Other options considered:
- `jitsi_meet` 4.0.0 is discontinued.
- `jitsi_meet_wrapper` 0.2.0+1 is about 3 years stale.
- `omni_jitsi_meet` and `custom_jitsi_meet` wrap the same full-UI SDK.
- `@jitsi/react-native-sdk` is the same UI SDK, without Flutter.
- None of these solves the UI problem.

### Why B

- **B is A's native core without A's presentation layer.** It keeps everything that's hard and that
  Jitsi maintains: WebRTC, the audio session, CallKit and ConnectionService, the iOS broadcast
  socket server, Android MediaProjection. And it lets Flutter decide where the view goes, if
  anywhere.
- **It mirrors the web's design one to one.**
  - Hidden view, room switching by remount.
  - Command surface: `setAudioMuted`, `toggleScreenShare`, `hangUp`.
  - Event surface: participant and share events.
  - So `JaasVoiceAdapter`'s logic ports almost line for line.
- **The shown-panel trick for screen share works the same way:** make the platform view visible and
  pin the sharer. Whether the native SDK can pin a participant from the outside is
  **UNVERIFIED**. The Flutter wrapper exposes no pin command; the RN SDK's `config` and `flags`
  may be enough, otherwise we accept Jitsi's dominant-speaker stage.

**Fallback if spike 1 fails** (hidden `JitsiMeetView` stops audio, or embedding on Android is
unworkable): use C for **voice only**.
- lib-jitsi-meet in a 1×1 WebView, audio only, attaching remote audio tracks to elements.
- Accept that backgrounding ends voice.
- Screen *viewing* through the same WebView, shown as a panel.
- Screen *sending* through B's native SDK only when sharing. Or skip it.

## 3. Architecture

```
┌──────────────────────────────── Flutter (Dart) ─────────────────────────────────┐
│  HUD widgets: mute, call list, share button, share panel frame                  │
│        ▲ state                                   │ commands                     │
│  VoiceController (port of Communication's output side + JaasVoiceAdapter)       │
│   - room id → "officexr-"+sha256[:32]   - token: GET /api/offices/:id/voice-token│
│   - joins/leaves, mute, share, roster → host (jitsiRoom / jitsiParticipantId)   │
│        ▲ HostEvent::VoiceRoom{room}           │ MethodChannel                   │
│        │ (existing EventChannel / plugin)     ▼                                 │
│  BevyView (Texture on Android / UiKitView on iOS)   JitsiView (platform view,   │
│        ▲                                            hidden 1×1 or shown panel)  │
└────────┼────────────────────────────────────────────────────┼───────────────────┘
         │ JNI / C API                                        │ Kotlin / Swift
┌────────┴──────── Rust host (mobile-flutter/native) ─────┐ ┌─┴─ Jitsi Meet SDK (native) ───┐
│ Bevy app + officexr-client (OfficeRoomPlugin)           │ │ JitsiMeetView (React Native)  │
│  officexr-protocol: positions, status incl. jitsiRoom   │ │ WebRTC, audio session,        │
│  officexr-sim PairTracker → proximity entered/exited    │ │ CallKit / ConnectionService   │
│  room derivation (zone > proximity, dwell, debounce)    │ │ Android: MediaProjection FGS  │
│    → HostEvent::VoiceRoom                               │ │ iOS: socket server ◀── Unix ──┼─┐
└──────────────────────────────────────────────────────────┘ └──────────▲───────────────────┘ │
                  realtime server (WS) ◀──── positions/status          │ WebRTC media         │
                                                                 JaaS 8x8.vc (JVB/jicofo)     │
                                         iOS ≤26: Broadcast Upload Extension (App Group) ─────┘
```

Where each concern lives:

| Concern | Lives in | Why |
|---|---|---|
| Proximity and zone membership | Rust (`officexr-sim::proximity::PairTracker`, already a port of the TS tracker) | The host owns positions every frame; one source of truth next to physics |
| Room decision (dwell, debounce, zone > proximity, lex-min) | Rust or Dart (see below) | Small state machine; must match `Communication` exactly |
| Room → conference name, JWT fetch, Jitsi commands, roster | Dart `VoiceController` | Platform-neutral; talks to the method channel of the Jitsi plugin |
| Media, audio session, call integration, capture | Native Jitsi SDK | Jitsi maintains it |
| Share viewing | The Jitsi platform view, shown as a Flutter-positioned panel | Same as web |

**Where the room decision lives.**
- **Recommendation: emit raw `proximity:entered/exited` and zone-presence changes from Rust, and
  port `Communication` (about 220 lines) to Dart.**
  - Rust then stays a pure world sim with no voice concepts. That respects the DIP direction the
    repo enforces: the sim knows nothing about media.
  - The Dart port can share test vectors with `core-refactor`'s `voice-zone-routing` and
    `communication` tests.
- The alternative is a Rust port of `Communication`. That is equally valid if a Rust desktop client
  will also need voice. Either way, the derivation functions (`deriveJitsiRoom`, `deriveZoneRoom`,
  the SHA-256 name) must be **byte-identical across TS, Rust and Dart**. A web user and a mobile
  user who derive different names would silently sit in different calls. Add a shared fixture file
  of `(officeId, self, nearby) → room → conference name` cases, checked by all three
  implementations.
- **Event path:** Rust `HostEvent` → JNI event thread → Kotlin listener → `EventChannel` (Android),
  or the iOS plugin's event sink → Dart. These are rare, low-rate events, so the existing channels
  are plenty.
- **Constraint:** on iOS, Bevy runs on the main thread. A slow frame delays platform-channel
  calls, including Jitsi commands. Mute latency should be measured.

## 4. Proximity audio on mobile

- **Binary proximity (web parity) is fully reproducible** with options A, B or C. Membership is
  computed locally from the same positions every client sees. Mobile needs:
  - the PairTracker running in the Rust host on the same settings (3 m / 6 m / 500 ms) — the
    `officexr-client` crate depends on `officexr-sim` already;
  - the status fields `jitsiRoom` and `jitsiParticipantId` sent through `officexr-protocol`.
- **Join latency:** each proximity change is a conference switch, so it means a new XMPP session
  and a new ICE connection on a mobile network. Expect a noticeably slower join than on desktop
  Wi-Fi (**UNVERIFIED**, measure). The 1.5 s leave debounce helps.
- **Distance-attenuated gain** is a new feature, not parity. It would need per-remote-track
  volume driven by distance: `Helper.setVolume` in flutter_webrtc (D/E), or a Web Audio GainNode
  (C).
  - The distance would come from Bevy: a `HostEvent::PeerDistances{(playerId, metres)}` at about
    10 Hz.
  - Mapping a Jitsi participant to a player uses the broadcast `jitsiParticipantId`.
  - B can't do it: the Jitsi SDK exposes no volume API. That's one more reason in-world media
    implies a stack change later.

## 5. Screen share on mobile

### Android: sending

The Jitsi SDK owns capture, so with B we mostly *configure*:
- **Manifest:** declare a foreground service with `foregroundServiceType="mediaProjection"` and
  `FOREGROUND_SERVICE_MEDIA_PROJECTION`. Targeting Android 14+ without the matching permission
  throws `SecurityException`. Jitsi's RN guide calls this `enableMediaProjectionService`; check
  how the native Android SDK exposes it.
- **Consent order:** `createScreenCaptureIntent()` → user consent → start the FGS →
  `getMediaProjection()`. Consent is single-use per session on Android 14+. Reusing it, or
  calling `createVirtualDisplay` twice, throws.
- **Single-app capture** is the default from Android 14. If the user shares only our app, they
  share a Bevy office, which is probably not what they meant. That's a UX hint, not a blocker.
- **Android 15 changes:**
  - notifications and OTPs are redacted during a share;
  - QPR1+ adds a status-bar chip to stop sharing;
  - projection stops on screen lock.
  - Handle `MediaProjection.Callback.onStop()`, which on our side means a share-stopped event to
    Dart.

### iOS: sending

- **Through iOS 26: ReplayKit.**
  - A Broadcast Upload Extension (no UI) plus an App Group shared by the app and the extension.
  - Info.plist keys `RTCAppGroupIdentifier` and `RTCScreenSharingExtension`.
  - The background modes `audio` and `voip`.
  - The extension streams frames over a Unix socket in the group container to the app, which
    sends them through WebRTC.
  - The extension's memory limit is about 50 MB according to forums and vendors (**UNVERIFIED**).
  - It's started with `RPSystemBroadcastPickerView`, using `preferredExtension`.
- **iOS 27: ScreenCaptureKit.** Apple's docs mark `RPSystemBroadcastPickerView` as deprecated and
  `RPBroadcastSampleHandler` as no longer supported. ScreenCaptureKit (`SCContentSharingPicker`)
  is the replacement, and "a broadcast extension is no longer necessary". Whether Jitsi's SDK or
  flutter_webrtc support this yet is **UNVERIFIED**. If they do, the sideload problem below goes
  away on iOS 27 devices.
- **Sideloaded free Apple ID builds:**
  - The free membership's capability table *includes App Groups* and background modes. It does
    *not* include Push, Push to Talk or Network Extensions.
  - A free account has 10 App IDs, each lasting 7 days, and 3 sideloaded apps per device.
  - **Every extension uses its own App ID**, so app + broadcast extension = 2 per install or
    refresh. AltStore's docs say this, and their source code shows they sign extensions.
  - AltStore also **rewrites every App Group to `<group>.<TEAMID>`** and writes the result into
    `ALTAppGroups` in Info.plist. A hard-coded `RTCAppGroupIdentifier` would no longer match the
    real container, so the app and extension must resolve the group at runtime.
  - How Sideloadly handles App Groups is **UNVERIFIED**. Reports say it *can* strip extensions.
  - Whether the extension counts toward the 3-app limit is **UNVERIFIED**.
  - **Conclusion:** possible in principle, fragile in practice. Needs spike 3 on a real device with
    a free Apple ID. A paid account ($99/yr) or TestFlight distribution removes the problem.
- **Our own UI:** the RPSystemBroadcastPickerView button can be styled or hidden behind our own
  button, but the system sheet always appears. That's fine.

### Viewing others' shares

- **Phase 1: show the Jitsi view as a panel** — the web's behaviour. No frame plumbing at all.
- **Later: in-scene video texture** (a share on a wall screen object). This needs frames, and so
  option D or E. The path for each platform:

  | | Path | Cost |
  |---|---|---|
  | Android, simple | libwebrtc `VideoSink` → `VideoFrame.toI420()` (a GPU→CPU readback when the buffer is an OES texture) → JNI → Rust → three R8 plane textures via `queue.write_texture` → YUV→RGB in a Bevy material shader | 1.5 B/px: 720p30 ≈ 41 MB/s, 1080p30 ≈ 93 MB/s of upload, plus the readback. Fine for 1–2 tiles; not for many. Never convert to RGBA on the CPU (about 2.7× the cost) |
  | Android, zero-copy | MediaCodec → ImageReader → `AHardwareBuffer` (API 28) → Vulkan import (`VK_ANDROID_external_memory_android_hardware_buffer`) → `wgpu_hal::vulkan::Device::texture_from_raw` → `Device::create_texture_from_hal` → swap it into `RenderAssets<GpuImage>` | wgpu 30 doesn't enable that extension, so we'd create the device ourselves (`open_with_callback` + `RenderCreation::Manual`). Vulkan YCbCr samplers aren't exposed by wgpu. Weeks of work, and a custom decoder sink in libwebrtc |
  | iOS | `RTCVideoFrame` → `RTCCVPixelBuffer` (usually NV12, **UNVERIFIED** per decoder) → `CVMetalTextureCache` → two `MTLTexture`s (R8 and RG8) → `wgpu_hal::metal::Device::texture_from_raw` → `create_texture_from_hal::<Metal>` → `GpuImage` swap → NV12→RGB shader | Near zero-copy, and the cheapest of the three. Needs Rust code with access to wgpu's Metal hal. Bevy runs on the main thread, so hand off textures on that thread |

- The `create_texture_from_hal` and `texture_from_raw` APIs were checked in the wgpu 30.0.2 and
  Bevy 0.20.1 source locked in this repo.
- Per CLAUDE.md, this would be a new primitive in `crates/officexr-render`, such as a
  `VideoSurface` component keyed by a stream id. It is fed by the host, and a later web
  implementation could use `importExternalTexture` the same way. A second canvas would not be
  allowed.

## 6. Camera video and avatars

- **Web has no camera video**, so there's no parity work. If it's wanted:
  - **With B:** turn on video in the Jitsi SDK and show its view — Jitsi's own filmstrip or tile
    UI inside our panel. Cheap, but it looks like Jitsi and can't sit on avatars.
  - **Flutter overlay tiles** (`RTCVideoView` per participant, placed by Flutter): needs D or E.
    For tiles that follow avatars, Bevy would emit the screen-space positions of heads (a
    `HostEvent` at frame rate, or better, a shared buffer read by Dart over FFI). Platform-view
    and texture composition costs grow with the tile count.
  - **In-world** (video on an avatar's face card or billboard): the same frame path as
    "in-scene share", times N participants at low resolution (for example 180p at 15 fps ≈
    0.6 MB/s each through the simple Android path, which is cheap).
    - Simulcast layer selection (bridge-channel receiver constraints) matters more than upload
      cost.
- **Recommendation:** no camera work before there's a product decision. If it comes, it's the
  trigger to move to D or E. At that point compare D's effort (very high) against moving web and
  mobile to LiveKit (medium, but a backend change).

## 7. Other mobile concerns

- **Push-to-talk:**
  - iOS's PushToTalk framework needs the Push Notifications and Push to Talk capabilities, which
    free Apple IDs can't have. It's also built around APNs wake-ups for walkie-talkie apps, not
    in-call PTT.
  - **Do in-app PTT instead:** a hold-to-talk HUD button that calls `setAudioMuted(false/true)`,
    the mobile version of the web's mute hotkey (`useMuteHotkey.ts`).
  - Android has no system PTT framework to use (**UNVERIFIED** that none exists).
- **Audio session and interruptions:**
  - The Jitsi SDK sets `playAndRecord` with the `voiceChat` mode (voice processing, echo
    cancellation; HFP is enabled implicitly).
  - Don't run a second audio engine (for example Bevy audio for UI sounds) that reconfigures the
    session. Play UI sounds through the same session, or not at all during calls.
  - Phone calls and Siri interrupt the session. iOS 27 adds new interruption lifecycle
    notifications. Surface "call interrupted" in the HUD and rejoin when the interruption ends.
- **Bluetooth headsets:**
  - On iOS, in `voiceChat` with an HFP and A2DP headset, **HFP wins, which means narrowband
    audio**. iOS 26's `bluetoothHighQualityRecording` isn't for real-time communication. Accept
    it.
  - On Android, use `setCommunicationDevice` (API 31+). `startBluetoothSco` is deprecated in
    API 34.
  - Check that the Jitsi SDK's audio-device picker works without its UI: the Flutter wrapper has
    no audio-route API (**UNVERIFIED**). We may need our own route button calling
    `AVRoutePickerView` or `setCommunicationDevice`.
- **Backgrounding:**
  - iOS forbids GPU work in the background: command buffers fail with `notPermitted`. **Pause the
    Bevy `CADisplayLink` on `didEnterBackground`** and call `waitUntilScheduled`. Voice can
    continue thanks to the `audio`/`voip` background modes.
  - Android: start a `microphone` (or `phoneCall`) FGS **when the user joins a call, while in the
    foreground**. Mic FGS can't be started from the background. Apps targeting Android 15 also
    need an FGS (or to be the top app) to get audio focus. Stop rendering when the
    SurfaceProducer is released; the prototype already restarts Bevy there.
  - **Proximity when backgrounded:** with the world paused, the local avatar stops but peers keep
    moving. Keep ticking `PairTracker` on the network thread without rendering, or freeze the
    current room while in the background. Freezing is simpler and avoids surprise joins from a
    pocketed phone. Recommended.
- **Battery and thermals:**
  - Rendering at 60 fps plus a WebRTC call is the main drain.
  - Cap Bevy at 30 fps while in a call.
  - Drop to a static frame or about 10 fps while the share panel covers most of the screen.
  - Pause when the screen is locked.
- **Call integration:**
  - CallKit doesn't need a special capability for outgoing or active calls. Incoming
    (PushKit/VoIP push) calls need Push, which free accounts lack. That's fine: we have no
    incoming calls.
  - Showing every proximity join as a "call" in the system call log would be odd. Consider
    disabling the call-integration flag and relying on background modes. Test which keeps audio
    alive better.
- **Network changes** (Wi-Fi to cellular): Jitsi handles ICE restarts. Our token refresh is per
  join, so a 1 h token with a 5 min margin is already fine.
- **Auth:** mobile must call `/api/offices/:id/voice-token` with the same session cookie that
  `officexr-client` uses (`OFFICEXR_SESSION_COOKIE`). There's no mobile sign-in flow yet; that
  belongs to the networking work, not this one.

## 8. Phased plan (smallest useful slice first)

0. **Spikes, about 1 week:**
   1. **Hidden `JitsiMeetView` in Flutter.** A minimal Android and iOS plugin that registers a
      platform view around `JitsiMeetView`, joins a JaaS room with a JWT pasted from the web's
      voice-token endpoint, then shrinks the view to 1×1 or `Offstage`. Pass criteria: two-way
      audio continues, and it survives backgrounding for 5 min. Also measure the APK and IPA size
      delta.
   2. **Conference switch latency** on a phone, over LTE and Wi-Fi.
   3. **iOS broadcast extension through Sideloadly** with a free Apple ID: does the App Group
      survive, does `RTCAppGroupIdentifier` match, and how many App IDs does it use? Also check
      whether Jitsi's iOS SDK 13.x supports ScreenCaptureKit on iOS 27.
1. **Voice parity (the first shippable slice):**
   - `HostEvent`s for proximity entered/exited and zone presence, from `PairTracker` in the Rust
     host.
   - Dart `Communication` port with shared fixtures, plus `VoiceController`.
   - The status fields `jitsiRoom` and `jitsiParticipantId` sent through the protocol.
   - Mute and hold-to-talk button, call list.
   - Pause rendering in the background; mic FGS on Android.
   - Cross-check: a web user and a mobile user walking up to each other land in the same
     conference.
2. **View screen shares:** show the Jitsi view as a panel while someone shares (from the
   `screenShareToggled` events); enlarge and shrink it.
3. **Send screen share on Android:** MediaProjection FGS configuration, stop chip, `onStop`
   handling.
4. **Send screen share on iOS:** behind spike 3's result. ScreenCaptureKit on iOS 27 if Jitsi
   supports it, else ReplayKit with runtime-resolved App Groups. Paid-account builds may be the
   honest answer.
5. **Later, product-driven:** camera video, in-world video textures, distance-based gain. These
   trigger the stack decision (D vs E), then the `VideoSurface` primitive in `officexr-render`
   (iOS first, the cheapest path), then the Android I420 path.

## 9. Open risks

- **Hidden native view stops media.** Spike 1 could fail. Fallback: option C for voice, without
  background audio.
- **Android `JitsiMeetView` embedding inside a `FlutterActivity`:** lifecycle forwarding through
  `JitsiMeetActivityDelegate`, React Native running alongside Flutter. Hybrid-composition platform
  views also add cost next to the Bevy `Texture`.
- **App size.** Flutter + Bevy + React Native + WebRTC + the Jitsi JS bundle in one app, size
  **UNVERIFIED**. Two JS-ish runtimes (Dart, Hermes/JSC) plus Rust.
- **Room-name drift across three implementations** (TS, Rust, Dart). Mitigate with shared
  fixtures in CI.
- **Proximity drift between web and mobile.** If the Rust `PairTracker` and the TS one diverge,
  two players disagree about whether they are in a call. The port claims event-order parity;
  needs a cross-impl test.
- **iOS screen sending with sideloading:** App ID budget, App Group rewriting, Sideloadly
  behaviour, and ReplayKit's deprecation in iOS 27.
- **JaaS terms for custom clients.** The 8x8 docs name only the IFrame API and mobile SDKs (fine
  for B). Formal support for lib-jitsi-meet (C) and fully custom clients (D) is **UNVERIFIED**.
- **JaaS cost.** Mobile users count as MAU. Developer plan: 25 MAU free, then $99/month for 300 MAU
  and up. The overage price differs between 8x8 pages (**UNVERIFIED**).
- **`moderator: true` for everyone** is more dangerous on mobile only if the SDK shows moderator
  UI. With no Jitsi UI it's unchanged from web.
- **iOS main-thread contention:** Bevy on the main thread delays Jitsi commands and
  platform-view work.
- **Unverified facts** listed inline: the ReplayKit extension memory limit, Sideloadly's App Group
  handling, whether native-SDK pinning works without UI, Jitsi or flutter_webrtc support for
  ScreenCaptureKit, WebView getDisplayMedia, NV12 formats per decoder.

## Sources

Code in this repo (branch `claude/jolly-galileo-gouq55`):
- `packages/core-refactor/src/communication/{communication,jitsi-room,zone-presence,call-roster-sync}.ts`
- `packages/web/src/voice/{JaasVoiceAdapter,JaasMeeting,jaasToken}.ts(x)`, `packages/web/src/hud/CallList.tsx`
- `packages/server/src/jaasToken.ts`, `packages/server/src/api.ts` (voice-token route)
- `packages/sdk/src/spatial/pair-tracker.ts`, `packages/sdk/src/game-state/world-constants.ts`
- `crates/officexr-sim/src/proximity.rs`, `schemas/protocol.schema.json`, `mobile-flutter/README.md`

Jitsi / JaaS:
- jitsi_meet_flutter_sdk: https://pub.dev/packages/jitsi_meet_flutter_sdk ·
  changelog https://pub.dev/packages/jitsi_meet_flutter_sdk/changelog ·
  repo https://github.com/jitsi/jitsi-meet-flutter-sdk
- Wrapper internals:
  [JitsiMeetPlugin.kt](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/android/src/main/kotlin/org/jitsi/jitsi_meet_flutter_sdk/JitsiMeetPlugin.kt),
  [WrapperJitsiMeetActivity.kt](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/android/src/main/kotlin/org/jitsi/jitsi_meet_flutter_sdk/WrapperJitsiMeetActivity.kt),
  [JitsiMeetPlugin.swift](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/ios/Classes/JitsiMeetPlugin.swift),
  [JitsiMeetViewController.swift](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/ios/Classes/JitsiMeetViewController.swift),
  [jitsi_meet.dart](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/lib/src/jitsi_meet.dart),
  [jitsi_meet_event_listener.dart](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/lib/src/jitsi_meet_event_listener.dart),
  [conference options](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/lib/src/jitsi_meet_conference_options.dart),
  [podspec](https://raw.githubusercontent.com/jitsi/jitsi-meet-flutter-sdk/main/ios/jitsi_meet_flutter_sdk.podspec)
- Feature flags: https://raw.githubusercontent.com/jitsi/jitsi-meet/master/react/features/base/flags/constants.ts
- Handbook:
  Android https://jitsi.github.io/handbook/docs/dev-guide/dev-guide-android-sdk ·
  iOS https://jitsi.github.io/handbook/docs/dev-guide/dev-guide-ios-sdk ·
  React Native https://jitsi.github.io/handbook/docs/dev-guide/dev-guide-react-native-sdk
- lib-jitsi-meet: https://github.com/jitsi/lib-jitsi-meet ·
  [JitsiTrack.ts](https://raw.githubusercontent.com/jitsi/lib-jitsi-meet/master/modules/RTC/JitsiTrack.ts) ·
  https://github.com/jitsi/ljm-getting-started ·
  https://raw.githubusercontent.com/jitsi/ljm-jaas-example/master/example.js
- SSRC rewriting / bridge channel: https://jitsi.org/blog/improving-performance-on-very-large-calls-introducing-ssrc-rewriting/ ·
  https://github.com/jitsi/jicofo
- JaaS: JWT https://developer.8x8.com/jaas/docs/api-keys-jwt ·
  onboarding https://developer.8x8.com/jaas/docs/jaas-onboarding ·
  pricing https://cpaas.8x8.com/sg/pricing/jitsi-as-a-service-pricing/
- Community wrappers: https://pub.dev/packages/jitsi_meet · https://pub.dev/packages/jitsi_meet_wrapper · https://pub.dev/packages?q=jitsi
- App-size data point (Jitsi Meet app, not SDK): https://forum.f-droid.org/t/jitsi-21-1-0-size/13650

WebRTC / WebView:
- flutter_webrtc: https://pub.dev/packages/flutter_webrtc ·
  iOS screen sharing https://github.com/flutter-webrtc/flutter-webrtc/wiki/iOS-Screen-Sharing ·
  [helper.dart](https://github.com/flutter-webrtc/flutter-webrtc/blob/main/lib/src/helper.dart)
- LiveKit: https://pub.dev/packages/livekit_client
- Dart XMPP packages (none implement Jingle): https://pub.dev/packages?q=xmpp
- WKWebView getUserMedia: https://bugs.webkit.org/show_bug.cgi?id=208788 ·
  getDisplayMedia https://bugs.webkit.org/show_bug.cgi?id=231455 ·
  per-element volume https://bugs.webkit.org/show_bug.cgi?id=241657 ·
  https://github.com/muxinc/media-chrome/discussions/913
- Android WebRTC screen share overview: https://www.forasoft.com/blog/article/android-webrtc-screen-sharing
- libwebrtc VideoFrame: https://webrtc.googlesource.com/src/+/refs/heads/main/sdk/android/api/org/webrtc/VideoFrame.java

Android:
- FGS types https://developer.android.com/develop/background-work/services/fgs/service-types ·
  FGS changes https://developer.android.com/develop/background-work/services/fgs/changes ·
  background starts https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start
- MediaProjection https://developer.android.com/media/grow/media-projection
- Android 15 https://developer.android.com/about/versions/15/behavior-changes-all ·
  https://developer.android.com/about/versions/15/behavior-changes-15
- AudioManager https://developer.android.com/reference/android/media/AudioManager ·
  Core-Telecom https://developer.android.com/develop/connectivity/telecom/voip-app/telecom

Apple:
- ReplayKit picker https://developer.apple.com/documentation/replaykit/rpsystembroadcastpickerview ·
  ScreenCaptureKit https://developer.apple.com/documentation/screencapturekit ·
  https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker
- Broadcast extension memory (forum): https://developer.apple.com/forums/thread/651367
- Capabilities per membership https://developer.apple.com/help/account/reference/supported-capabilities-ios ·
  memberships https://developer.apple.com/support/compare-memberships/
- AltStore App IDs https://faq.altstore.io/altstore-classic/app-ids ·
  [FetchProvisioningProfilesOperation.swift](https://github.com/altstoreio/AltStore/blob/master/AltStore/Operations/FetchProvisioningProfilesOperation.swift) ·
  Sideloadly FAQ https://sideloadly.io/faq
- Audio: voiceChat mode https://developer.apple.com/documentation/avfaudio/avaudiosession/mode-swift.struct/voicechat ·
  allowBluetoothA2DP https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/allowbluetootha2dp ·
  bluetoothHighQualityRecording https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/bluetoothhighqualityrecording ·
  interruptions https://developer.apple.com/documentation/avfaudio/handling-audio-interruptions
- Metal in background https://developer.apple.com/documentation/metal/preparing-your-metal-app-to-run-in-the-background
- CallKit https://developer.apple.com/documentation/callkit
- PiP for video calls https://developer.apple.com/documentation/avkit/adopting-picture-in-picture-for-video-calls ·
  https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.avfoundation.multitasking-camera-access
- PushToTalk https://developer.apple.com/documentation/pushtotalk ·
  https://developer.apple.com/documentation/pushtotalk/creating-a-push-to-talk-app

wgpu / Bevy (checked in the crate sources this repo locks: wgpu 30.0.2, Bevy 0.20.1):
`wgpu::Device::create_texture_from_hal`, `wgpu_hal::{metal,vulkan,gles}::Device::texture_from_raw`,
`bevy_render::settings::RenderCreation::Manual`, `RenderAssets<GpuImage>`.
