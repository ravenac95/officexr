# OfficeXR on iOS

`OfficeXR.xcodeproj` wraps `officexr-client` the way Bevy 0.20's `examples/mobile` does: a
`Rust` build phase runs `build_rust_deps.sh`, which `cargo build`s the client's binary for the
simulator or device triple and `lipo`s it in as the app's executable. There are no Swift or
Objective-C sources.

On the device the bottom-left stick walks (a near-full push runs) and dragging anywhere else
looks. The first launch shows a config screen; **Save and quit** writes it to the app's
`Documents/officexr-config.json` and quits, and the next launch joins the room. If joining fails
the screen comes back with the error. Env vars set on the Xcode scheme (`OFFICEXR_WEB_URL` and the
rest, see `src/lib.rs`) win over the saved config.

CI builds the simulator target unsigned (`ios` job in `.github/workflows/ci.yml`). Device signing
and TestFlight are manual.

## Manual check (a person on a Mac)

Needs Xcode 16+, rustup, and an iPhone on the same Wi-Fi as the Mac.

1. **Rust targets.** From the repo root:
   `rustup target add aarch64-apple-ios-sim aarch64-apple-ios`.
2. **LAN dev server.** From the repo root: `pnpm install`, then `pnpm dev -- --host` so Vite
   listens on `0.0.0.0:5173`. Find the Mac's LAN IP (`ipconfig getifaddr en0`, e.g.
   `192.168.1.20`). Check that `http://192.168.1.20:5173` opens in Safari on the phone.
3. **Session cookie.** Sign in on the Mac's browser at `http://192.168.1.20:5173`, open an office
   and copy its id from the URL. In DevTools → Network, copy the request `Cookie` header of any
   `/api/me` request.
4. **Simulator.** `open crates/officexr-client/ios/OfficeXR.xcodeproj`, pick the `OfficeXR`
   scheme and an iPhone simulator, press Run (the first build takes several minutes).
   On the config screen enter:
   - Web URL: `http://192.168.1.20:5173` (`http://localhost:5173` also works in the simulator)
   - Realtime URL: the one the desktop client uses, with `localhost` replaced by the LAN IP
   - Office id: from step 3
   - Session cookie: from step 3 (paste with a long press, or Edit → Paste in the simulator)

   Tap **Save and quit**, then launch OfficeXR again from the simulator's home screen.
5. **Device.** Plug in the iPhone and enable Developer Mode. In the `OfficeXR` target's
   *Signing & Capabilities*, tick *Automatically manage signing*, choose your team and, if Xcode
   asks, change the bundle id from `dev.officexr.client`. Pick the phone as the run destination
   and press Run. Allow *Local Network* access when iOS asks. Fill in the config screen as in
   step 4, with the LAN IP.
6. **Web observer.** Open the same office in a browser on the Mac, signed in as a *different*
   user (a private window).

Capture these screenshots (simulator: ⌘S; device: side + volume-up) and attach them to the
pull request:

1. The config screen, filled in (blur the cookie).
2. The room in the simulator after joining, with the stick visible bottom-left.
3. The simulator mid-walk while holding the stick, after dragging to turn the view.
4. The web observer at the same moment, showing the native avatar moving.
5. Steps 2–4 again on the device.
