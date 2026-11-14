# Rust crates

Every Rust crate lives in `crates/<name>/` and is picked up by the root `Cargo.toml` workspace (`members = ["crates/*"]`). The toolchain is pinned in `rust-toolchain.toml`.

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace -- -D warnings
```

Dev builds use `opt-level = 1` for workspace crates and `3` for dependencies. On Linux the linker is `lld` via `clang`: install both (`apt install clang lld`).

Bevy crates should enable Bevy's `dynamic_linking` feature for local dev builds only (`cargo run --features bevy/dynamic_linking`); release and CI builds must link statically.

The wire protocol is exported from the zod schemas to `schemas/protocol.schema.json` (`pnpm --filter @officexr/realtime-server gen:protocol-schema`); `crates/officexr-protocol` generates its types from that file at build time (`build.rs`) and ships a websocket client. Its round-trip fixtures in `tests/fixtures/` come from `pnpm --filter @officexr/realtime-server gen:rust-fixtures`; rerun it when the protocol changes. Its integration test spawns the real realtime server with `node --import tsx`, so run `pnpm install` first.

`crates/officexr-sim-wasm` wraps `officexr-sim` for the browser. `pnpm --filter @officexr/sim-wasm build` (needs the `wasm32-unknown-unknown` target from `rust-toolchain.toml` and [`wasm-pack`](https://github.com/rustwasm/wasm-pack) 0.15.1) writes it to `packages/sim-wasm/pkg`, which `@officexr/world` imports, so build it before `pnpm typecheck`, `pnpm build` or `pnpm test`. The web app and studio run the local player on it with `?sim=rust`; TypeScript (`TsSimCore`) stays the default.

`crates/officexr-render-web` runs `officexr-render` in a browser canvas. It has two mutually exclusive cargo features, `webgpu` and `webgl2`, neither on by default. `pnpm --filter @officexr/render-wasm build` builds both into `packages/render-wasm/pkg/<backend>/` (wasm-pack fetches binaryen for its `wasm-opt -Oz` pass); the studio loads it with `loadRenderer()`, so build it alongside the sim wasm. The studio's `?op=bevy` draws the current map with it, and `?op=bevy&mugshot=<character>` the Mugshot scene.

`crates/officexr-controls` is a Bevy plugin for keyboard and mouse input and the gameplay camera modes (first-person, third-person, fixed, top-down, free-fly), ported from `CameraRig.tsx` and `lookControls.ts`. It keeps one `Controls` per view in its `ControlsHub`: each frame it turns held keys into a camera-relative `MoveIntent` and writes the view's camera. `officexr-render-web` exposes it as `takeIntent`, `setFollowTarget`, `setCameraMode`, `setConversationFocus`, `setCameraConfig` and `setInputEnabled`.
