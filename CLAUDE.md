# CLAUDE.md — agent guardrails

Project orientation lives in [`AGENTS.md`](./AGENTS.md). The architecture
overview lives in [`ARCHITECTURE.md`](./ARCHITECTURE.md). The active
refactor design docs live under [`refactor-plan/`](./refactor-plan/).
Read those first if you're new to the codebase.

This file captures things that aren't about *what* the system does, but
about *how* code in it should be written. Keep it short.

## SOLID is the default architecture

Default to writing code that respects the five SOLID principles. They're
not aspirational rules — they're the baseline the existing
`@officexr/sdk` + `@officexr/world` + `@officexr/studio` split is built around, and the
`refactor-plan/` work restored them where the old (since removed)
`RoomScene.tsx` god component eroded them.

- **Single Responsibility.** A module, class, or component does one
  thing. If you can't describe its job in one short sentence without
  using "and", split it. Concrete examples in this repo: `SyncEngine`
  delegates to `PositionBroadcaster` + `StateDiffBroadcaster` +
  `InboundRouter`; `BotDriver` delegates to `BotPhysicsWorld` + per-mode
  strategy files in `bot/modes/`.
- **Open/Closed.** Prefer adding a new file (a new mode strategy, a new
  panel, a new `NetEvent` kind) to editing an existing switch/giant
  function. Strategy tables (`BOT_MODE_STRATEGIES: Record<BotMode, …>`)
  and the `PROTOCOL` table are existing examples — they fail at compile
  time when a new variant is added without a registry entry.
- **Liskov Substitution.** A concrete `Channel` implementation
  (`InMemoryChannel`, `WsChannel`) must obey the
  same observable contract the `Channel` interface advertises —
  including subscribe / send / presence semantics. Any "this
  implementation behaves slightly differently" needs to either be
  surfaced in the interface or eliminated.
- **Interface Segregation.** Don't make a caller depend on more than it
  uses. `WorldSettings` is now an intersection of `AnimationSettings &
  ProximitySettings & CollisionSettings & …` precisely so a function
  that only needs movement tunables can ask for `AnimationSettings &
  CollisionSettings` instead of the whole bag.
- **Dependency Inversion.** High-level modules don't depend on
  low-level details. `@officexr/sdk` and `@officexr/core-refactor` know
  nothing about the renderer or `react`; `@officexr/world` (Bevy views +
  physics + bots) depends on the SDK, never the reverse.
  This is enforced by greps that should stay clean:

  ```bash
  grep -rn "@officexr/render-wasm" packages/sdk packages/realtime-server packages/core-refactor
  grep -rn "from 'react'" packages/sdk packages/realtime-server packages/core-refactor
  ```

  Both must return zero matches.

## When you violate SOLID, document the reason inline

Some violations are deliberate — a framework constraint, a perf budget,
a behaviour-preserving shim during migration. That's fine. **What's not
fine is a silent violation.** When you knowingly bend one of the rules
above, leave a comment at the violation site that says:

1. **Which principle** is being bent (SRP / OCP / LSP / ISP / DIP).
2. **Why** — the constraint that forced the choice (existing API,
   third-party library shape, perf, migration step, etc.).
3. **What would need to change** to remove the violation, if anything.

Example shape (don't copy verbatim, adapt to the situation):

```ts
// DIP violation: `bootBevy` keeps the renderer in a module-level
// promise instead of taking it as an injected dependency. Bevy owns
// its event loop for the page's lifetime and boots once per page, so
// there is never a second instance to inject. Removing this needs a
// Bevy that can boot and shut down per view.
```

Comments like that turn a code smell into a reviewed, traceable
decision. Comments like `// hack` or no comment at all turn it into
tech debt that compounds.

If a SOLID violation has *no* good reason — i.e. you'd just rather not
do the split — fix it instead of documenting it. The doc-the-reason
escape hatch is for genuine constraints, not for skipping refactors.

## Other guardrails

- **No mock-only tests for cross-process behaviour.** If a test would
  pass with mocked channels but fail in production,
  add an integration test against the in-memory channel hub
  (`createInMemoryChannelHub`) — that's the convergence-test pattern
  the SDK already uses.
- **No three.js, react-three-fiber or Leva.** Every surface renders
  through Bevy: `crates/officexr-render`, loaded in the browser as
  `@officexr/render-wasm` and mounted by the views in
  `packages/world/src/react/` (`BevyScene`, `BevyPreview`, `bootBevy`).
  Hooks and SDK code stay headless; rendering logic belongs in Rust.
  `pnpm lint:no-three` (run in CI) fails on any `three`,
  `@react-three/*` or `leva` import or dependency.
- **No new `useRef + setStateSync` mirror pairs.** They were the
  load-bearing footgun that ARCHITECTURE.md observation O3 calls out.
  Use the store or a `subscribeAll` listener instead.
- **Editor views compose `officexr-render` primitives; they don't
  re-implement them.** An editor mounts a Bevy view and drives it
  through `@officexr/render-wasm`: the world (instances, baked
  layouts, lighting, sky) through `setWorld`, editor-only drawing
  (selection outlines, tool ghosts, grids, gizmos, markers, zones,
  labels) through `setOverlays`, and hit-testing through picking
  (`onPick`, `rayAtScreen`, `intersectPlane`). Character and kind
  previews mount `<BevyPreview />`. DOM UI (toolbars, tooltips,
  menus) sits beside the canvas. If you need a new primitive, add
  it to `crates/officexr-render` (a new `Overlay` kind, say) and
  expose it through `officexr-render-web` — don't draw it in a
  second canvas inside an editor. No lint checks this; it is
  enforced in review.
- **Hermetic studio test mode (`?test=1`).** `packages/studio/src/App.tsx`
  has a startup branch: when the URL carries `?test=1`, the studio boots
  with a bundled-catalog `ApplicationApi` (no `/api/world-object-kinds`
  fetch) and in-memory storages (no `/api/*` filesystem), seeded from
  `window.__OFFICEXR_TEST_SEED__`. This is a deliberate production-code
  seam for the Playwright suite, not a stealth feature — it's a single
  branch evaluated once at boot and is null/no-op in normal operation.
  The editor document hooks resolve storage as
  `explicit option → TestStorageContext (?test=1) → Filesystem stack`.
  The hermetic harness + conventions live under
  `packages/studio/src/test-harness/` (see its README). When adding a
  new editor hook that persists, accept an optional `storage` and read
  `useTestStorages()` so it stays hermetically testable.
- **Renderer changes MUST be run against their visual contract.**
  - `crates/officexr-render` (Bevy): `cargo test -p officexr-render
    --features visual-tests [<scenario>]` renders the scenarios in
    `crates/officexr-render/tests/visual/scenarios/` headless, asserts
    the sim's positions (feet within 10 cm of the surface, character
    spacing) and diffs keyframes against
    `crates/officexr-render/tests/visual/baselines/`. Output lands in
    `target/tmp/visual-tests/`. It needs a Vulkan adapter (Mesa
    lavapipe: `apt install mesa-vulkan-drivers`) and
    `pnpm asset-packs:install`; so does `cargo test --workspace
    --all-features`, which runs it. A missing baseline fails;
    `OFFICEXR_UPDATE_BASELINES=1` writes them. CI renders on
    ubuntu-24.04 lavapipe, so regenerate on linux and review each PNG
    by hand before committing.
  - Playwright only smoke-tests rendering: `rendering-smoke.spec.ts`
    checks each surface boots Bevy and draws a non-blank canvas. It
    holds no baseline PNGs; pixels belong in the native harness above.
  - Never widen a diff threshold to make a suite green.
