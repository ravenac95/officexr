# WebXR + Bevy spike (OSO-7951). Throwaway, don't merge.

Findings are in `docs/webxr-bevy.md` on the OSO-7951 branch. This page boots
the webgl2 renderer on a canvas, enters `immersive-vr` and drives Bevy from
`XRSession.requestAnimationFrame` through `xrFrame` (`crates/officexr-render-web/src/xr.rs`).

```bash
CARGO_INCREMENTAL=0 cargo build -p officexr-render-web --target wasm32-unknown-unknown --features webgl2 --release
wasm-bindgen --target web --out-dir spikes/webxr-bevy/pkg target/wasm32-unknown-unknown/release/officexr_render_web.wasm
cd spikes/webxr-bevy && python3 -m http.server 8765
# headless check (IWER emulates a Quest 3; screenshots land in the given dir):
node run-xr.cjs "$PWD"
```

Open `http://localhost:8765/index.html?emulate` and call `enterVR()` in the
console, or open it without `?emulate` on a headset browser (real
`XRWebGLLayer.framebuffer`, so the blit path runs: untested).
