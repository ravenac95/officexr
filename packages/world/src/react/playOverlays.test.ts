import { describe, expect, it } from 'vitest';
import { missionBeacon, placementOverlays } from './playOverlays.ts';

const ghost = (valid: boolean) => ({ kindId: 'prop_pool' as const, position: [4, 2, 6] as [number, number, number], yawDeg: 90, valid });
const box = { min: [8, 0, 8] as [number, number, number], max: [10, 1, 10] as [number, number, number] };

describe('missionBeacon', () => {
  it('stands on the floor at the target, captioned with the metres to go', () => {
    const beacon = missionBeacon({ x: 3, z: 4, label: 'Stand-up' }, { x: 0, z: 0 });
    expect(beacon).toMatchObject({ kind: 'beacon', at: [3, 1, 4], label: 'Stand-up · 5 m' });
    expect(missionBeacon({ x: 3, z: 4, label: 'Stand-up' }, null)).toMatchObject({ label: 'Stand-up' });
  });
});

describe('placementOverlays', () => {
  it('shows nothing without a ghost', () => {
    expect(placementOverlays({ ghost: null, target: null })).toEqual([]);
  });

  it('ghosts the chosen prop green where it fits and red where it does not', () => {
    const [fits] = placementOverlays({ ghost: ghost(true), target: null });
    expect(fits).toMatchObject({
      kind: 'ghost',
      objects: [{ kindId: 'prop_pool', position: [4, 2, 6], yawDeg: 90 }],
      tint: '#2ee59d',
      pickable: false,
    });
    expect(placementOverlays({ ghost: ghost(false), target: null })[0]).toMatchObject({ tint: '#ff4d4d' });
  });

  it('outlines the placed prop the player points at', () => {
    const overlays = placementOverlays({ ghost: ghost(true), target: box });
    expect(overlays[1]).toMatchObject({ kind: 'outline', boxes: [box] });
  });
});
