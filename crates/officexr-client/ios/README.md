# OfficeXR on iOS

`OfficeXR.xcodeproj` wraps `officexr-client` the way Bevy 0.20's `examples/mobile` does: a
`Rust` build phase runs `build_rust_deps.sh`, which `cargo build`s the client's binary for the
simulator or device triple and `lipo`s it in as the app's executable. There are no Swift or
Objective-C sources.

On the device the bottom-left stick walks (a near-full push runs) and dragging anywhere else
looks. The config normally comes from env vars on the Xcode scheme (`OFFICEXR_WEB_URL` and the
rest, see `src/lib.rs`); Xcode passes them to the app on the simulator and on a device. Without
them, the app falls back to a saved config, then to an in-app config screen. **Save and close**
writes it to `Library/Application Support/officexr-config.json` (the cookie in plaintext) and
ends the process; the next launch joins the room. If joining fails the screen comes back with the
error. The screen is for editing only: pasting from the system clipboard into it is not supported,
so a session cookie can't realistically be entered there.

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
   `/api/me` request. Sign in on the same host you enter as the Web URL below: the cookie is tied
   to that host.
4. **Scheme env vars.** `open crates/officexr-client/ios/OfficeXR.xcodeproj`, then
   *Product → Scheme → Edit Scheme… → Run → Arguments → Environment Variables* and add:
   - `OFFICEXR_WEB_URL`: `http://192.168.1.20:5173`
   - `OFFICEXR_REALTIME_URL`: the one the desktop client uses, with `localhost` replaced by the
     LAN IP
   - `OFFICEXR_OFFICE_ID`: from step 3
   - `OFFICEXR_SESSION_COOKIE`: from step 3

   Leave *Shared* unticked so the cookie stays out of the repo.
5. **Simulator.** Pick the `OfficeXR` scheme and an iPhone simulator and press Run (the first
   build takes several minutes). The app joins the room straight away.
6. **Device.** Plug in the iPhone and enable Developer Mode. In the `OfficeXR` target's
   *Signing & Capabilities*, tick *Automatically manage signing*, choose your team and, if Xcode
   asks, change the bundle id from `dev.officexr.client`. Pick the phone as the run destination
   and press Run. Allow *Local Network* access when iOS asks. The scheme env vars from step 4
   apply here too.
7. **Web observer.** Open the same office in a browser on the Mac, signed in as a *different*
   user (a private window).

Capture these screenshots (simulator: ⌘S; device: side + volume-up) and attach them to the
pull request:

1. The config screen (unset the scheme env vars and delete the app first), blurring any cookie.
2. The room in the simulator after joining, with the stick visible bottom-left.
3. The simulator mid-walk while holding the stick, after dragging to turn the view.
4. The web observer at the same moment, showing the native avatar moving.
5. Steps 2–4 again on the device.
