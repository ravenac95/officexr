# Studio test harness

Hermetic helpers for testing the studio editors without a dev server,
a database, or the network. Three layers, mirroring the test pyramid in
the integration-test roadmap.

## What's here

- **`createTestApi()`** — composes an `ApplicationApi` via the
  production `createDefaultApi` with `apiPath: ''` (bundled default
  catalog, no `/api/world-object-kinds` fetch) and the default bake loader, which throws if a test triggers a bake
  (pass `loadGltf` to opt in).
- **`StudioHarness`** — wraps children in an `<ApplicationProvider>`
  built from `createTestApi()`. Use as the `wrapper` for `renderHook`
  on any hook that reads `useApplication` (e.g. `useRoomDocument`,
  `useLayoutDocument`).
- **`renderEditorStore({ stagedKindId? })`** — headless scene editor: the
  Room document store plus tool dispatch, no renderer. Drive it with
  `setTool`, `setBuildHeight`, `pick({ target, point })` and
  `moveSelection(delta)`. Placement and move occupancy use the same pure
  functions as the canvas (`resolvePlacement`, `checkMoveOccupancy`).
- **`seeds/`** — fixture documents (`emptyMapSeed`, `simpleRoomSeed`,
  `twoRoomMapSeed`). Pair with an `InMemory*Storage` (from
  `@officexr/world/scenes`) to seed a hook hermetically.

## Tier 2 — document hook tests

`renderHook` the hook against an in-memory storage. No canvas.

```ts
const storage = new InMemoryMapStorage({ seed: [{ name: 'default', doc }] });
const { result } = renderHook(() => useMapDocument({ storage }));
await waitFor(() => expect(result.current.doc.name).toBe('default'));
```

For hooks that read `useApplication`, pass `{ wrapper: StudioHarness }`.

## Tier 3 — editor store and tools

`renderEditorStore` drives the scene editor's store and tool dispatch
with picks expressed as data, no renderer:

```ts
const editor = await renderEditorStore({ stagedKindId: '__primitive_blue' });
editor.setTool('add');
editor.pick({ target: { type: 'empty' }, point: { x: 0, z: 0 } });
```

Use primitive kinds (`__primitive_blue` / `__primitive_stone`) in
fixtures; real catalog kinds load a GLTF over HTTP, which fails
hermetically. The map editor test (`MapApp.scenarios.test.tsx`) still has
its own r3f harness next to it.

## Coverage map (what tests what)

| Regression class | Where it's caught |
|---|---|
| A pointer hit data | `MapApp.scenarios` (spawn hit point), `roomSnap` unit |
| B batch closure | `useRoomDocument` (placeMany+groupCommands) |
| C hardcoded fallback | `useMapDocument` (spawn position), `MapApp.scenarios` |
| D ref/module staleness | `object-editor.spec.ts` kind-switch (Tier 4) |
| E cross-hook desync | `useRoomDocument` (setLayoutName), `useLayoutDocument` (optimizer) |
| F implicit gating | `mapPointerActions` unit + `MapApp.scenarios` (litmus) |

Tier-3 scenarios: `scene-editor/__tests__/editorStore.test.tsx` covers
placement, selection, move, delete and undo through `renderEditorStore`;
`MapApp.scenarios` covers the Map editor (the c606f10 class). Room tile-grouping is covered at Tier 2; the Object-editor Leva
kind-switch (a DOM concern, not a scene-graph one) is covered by the
existing `object-editor.spec.ts` at Tier 4.

## Tier 4 — Playwright hermetic mode (`?test=1`)

`App.tsx` boots hermetically when the URL carries `?test=1`: a
bundled-catalog api (no `/api/world-object-kinds` fetch) and in-memory
storages seeded from `window.__OFFICEXR_TEST_SEED__`. The
`goToModeHermetic(page, mode, seed)` helper sets the seed via
`addInitScript` and navigates. See `tests/playwright/map-editor-hermetic.spec.ts`.

Sounds mode reads its in-memory `InMemorySoundLibraryStorage` from the
`soundStorage` entry of `TestStorages`; seed it with
`sounds: [{ name, bytes: number[] }]` (see `tests/playwright/sounds-editor.spec.ts`),
`modelSourcesClient` (a `ModelSourcesClient` object, set from an `addInitScript` function since it is not JSON; see `tests/playwright/model-upload-hermetic.spec.ts`).

In test mode `window.__OFFICEXR_TEST_API__.worldConstantsSaves` lists every
document saved to the world-constants storage, so a spec can assert what the
Debug panels saved (see `tests/playwright/debug-world-constants-hermetic.spec.ts`).
Non-hermetic Debug specs get the defaults: the dev server does not seed
`world-settings.json` into its scratch data dir (`seed: false`), so a developer's
gitignored `packages/world/world-settings.json` is never loaded.

**CI** runs `pnpm test:e2e:ci`, now scoped to: `map-editor`,
`map-editor-hermetic`, `object-editor`, `routing`, `room-editor`,
`debug-mode`, `character-on-surface`, `debug-world-constants-hermetic`,
`render-page`, `render-view-mcp`, `sounds-editor`, and `model-upload-hermetic`.

The previously-rotted specs are repaired:

- `object-editor.spec.ts` / `routing.spec.ts` now query the renamed
  `/api/world-object-kinds` endpoint (was `/api/cube-kinds`).
- `room-editor.spec.ts` reveals the "Show layout objects" toggle before
  staging `Blue block` (a layout object, hidden by default), and
  expects the current five-tool toolbar (Select/Add/Delete/Tile/Move).
- `debug-mode.spec.ts` was realigned to the baked-geometry model: the
  map-switch test asserts a new `/api/baked-layouts/<layout>` fetch +
  pixel-hash change (maps carry no inline store cubes anymore), and the
  cube-field regression grid is sized to the global `VOXEL_SIZE=0.5`
  (the geometry service ignores a snapshot's `cubeSize`).

`character-on-surface.spec.ts` uses Playwright's per-platform
`toHaveScreenshot`. Its stale `SETTLED_BODY_Y` was fixed (cube tops
moved y=2→y=1 when `cubeSize` aligned to `VOXEL_SIZE`), and BOTH
platforms' baselines were regenerated and hand-verified (character
stands on the surface): the `-darwin.png` set locally, and the
`-linux.png` set in an amd64 Playwright container via podman (see
CLAUDE.md — the committed `-linux.png` set is what CI compares against,
so it must come from a linux/amd64 render, not a macOS dev box).
