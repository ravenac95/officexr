import { afterEach, describe, expect, it } from 'vitest';
import type { OcclusionInput, Overlay, ScreenInput } from '@officexr/render-wasm';
import type { Store } from '@officexr/sdk';
import { createPlaceMode, type PlacementHost } from '../motion/place-mode.ts';
import { WorldEffects, type EffectsView, type WorldEffectsOptions } from './worldEffects.ts';

type MarkerWindow = { __OFFICE_OCCLUSION__?: { fading: number; cutaway: number } };

function setup(options: Partial<WorldEffectsOptions> = {}) {
  const state = {
    players: { me: { id: 'me', pos: { x: 0, y: 1, z: 0 } } },
    worldSettings: { charRadius: 0.4 },
    placedProps: [],
  };
  const sent = {
    occlusion: [] as (OcclusionInput | null)[],
    screens: [] as ScreenInput[][],
    overlays: [] as Overlay[][],
  };
  const view: EffectsView = {
    setOcclusion: (input) => sent.occlusion.push(input),
    occlusionStats: () => ({ fading: 2, cutaway: 1, silhouetteSuppressed: [] }),
    setScreens: (screens) => sent.screens.push(screens),
    setOverlays: (overlays) => sent.overlays.push(overlays),
  };
  const effects = new WorldEffects(
    { store: { getState: () => state } as unknown as Store, selfId: 'me', view },
    {
      cameraMode: 'fixed',
      water: [],
      deskScreens: [],
      missionMarker: null,
      geometry: {
        voxelSize: 0.5,
        worldAABB: () => ({ min: [0, 0, 0], max: [1, 1, 1] }),
        worldAABBOfInstance: () => ({ min: [0, 0, 0], max: [1, 1, 1] }),
      },
      ...options,
    },
  );
  return { effects, sent, state };
}

afterEach(() => {
  delete (globalThis as MarkerWindow).__OFFICE_OCCLUSION__;
});

describe('WorldEffects', () => {
  it('sends occlusion every frame while the camera looks on, then clears it once', () => {
    const { effects, sent } = setup();
    effects.step(0.016);
    effects.step(0.016);
    expect(sent.occlusion).toHaveLength(2);
    expect(sent.occlusion[0]?.cutaway).toEqual([0, 1.5, 0]);
    expect((globalThis as MarkerWindow).__OFFICE_OCCLUSION__).toEqual({ fading: 2, cutaway: 1 });

    effects.options = { ...effects.options, cameraMode: 'first-person' };
    effects.step(0.016);
    effects.step(0.016);
    expect(sent.occlusion.slice(2)).toEqual([null]);
    expect((globalThis as MarkerWindow).__OFFICE_OCCLUSION__).toEqual({ fading: 0, cutaway: 0 });
    effects.dispose();
  });

  it('sends the overlays only when they change: the mission beacon and the placement ghost', () => {
    const placeMode = createPlaceMode();
    const placement = { mode: placeMode, send: () => Promise.reject(new Error('unused')) } as PlacementHost;
    const { effects, sent, state } = setup({ missionMarker: { x: 3, z: 4, label: 'Go' }, placement });
    effects.step(0.016);
    effects.step(0.016);
    expect(sent.overlays).toHaveLength(1);
    expect(sent.overlays[0].map((o) => o.kind)).toEqual(['beacon']);

    placeMode.setOpen(true);
    placeMode.setGhost({ position: [2, 2, 2], valid: true, targetId: null });
    state.players.me.pos = { x: 3, y: 1, z: 0 };
    effects.step(0.016);
    expect(sent.overlays[1].map((o) => o.kind)).toEqual(['beacon', 'ghost']);
    expect(sent.overlays[1][0]).toMatchObject({ label: 'Go · 4 m' });
    effects.dispose();
  });
});
