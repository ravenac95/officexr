import { describe, expect, it } from 'vitest';
import type { Overlay } from '@officexr/render-wasm';
import { mapOverlays, type MapOverlayInputs } from '../mapOverlays.ts';

const area = { id: 'd1', min: { x: 0, z: 0 }, max: { x: 2, z: 4 } };
const base: MapOverlayInputs = {
  voiceZones: [],
  deskAreas: [area],
  violations: [],
  spawns: [{ id: 's1', position: [1, 2, 3] }],
  selection: null,
  selectedRoom: null,
  draft: null,
  deskAreasPickable: true,
};
const byTag = (overlays: Overlay[], tag: string) => overlays.find((o) => o.tag === tag);

describe('mapOverlays', () => {
  it('draws a desk area as a pickable zone with its label at its centre', () => {
    const overlays = mapOverlays(base);
    expect(byTag(overlays, 'desk:d1')).toMatchObject({
      kind: 'zone',
      pickable: true,
      color: '#22d3ee59',
      shape: { type: 'polygon', points: [[0, 0], [2, 0], [2, 4], [0, 4]] },
    });
    expect(byTag(overlays, 'desk-label:d1')).toMatchObject({ kind: 'label', at: [1, 0.06, 2], text: 'Desk' });
    expect(byTag(mapOverlays({ ...base, deskAreasPickable: false }), 'desk:d1')).toMatchObject({ pickable: false });
  });

  it('colours a desk area red when it breaks the rules, yellow when selected', () => {
    const selected = { ...base, selection: { kind: 'desk-area' as const, id: 'd1' } };
    expect(byTag(mapOverlays(selected), 'desk:d1')).toMatchObject({ color: '#fde04759' });
    const invalid = { ...selected, violations: [{ areaId: 'd1', label: 'Desk', reason: '2 chairs' }] };
    expect(byTag(mapOverlays(invalid), 'desk:d1')).toMatchObject({ color: '#ef444459' });
  });

  it('draws spawns as cones, marking the selected one', () => {
    expect(byTag(mapOverlays(base), 'spawn:s1')).toMatchObject({ kind: 'marker', shape: 'spawnCone', pos: [1, 2, 3], selected: false });
    const selected = mapOverlays({ ...base, selection: { kind: 'spawn', id: 's1' } });
    expect(byTag(selected, 'spawn:s1')).toMatchObject({ selected: true });
  });

  it('adds voice zones, the draft and the selected room only when there are any', () => {
    expect(mapOverlays(base).map((o) => o.tag)).toEqual(['grid', 'desk:d1', 'desk-label:d1', 'spawn:s1']);
    const overlays = mapOverlays({
      ...base,
      voiceZones: [{ id: 'r1', min: { x: 0, z: 0 }, max: { x: 1, z: 1 } }],
      draft: { min: { x: 4, z: 4 }, max: { x: 5, z: 5 } },
      selectedRoom: { min: [0, 0, 0], max: [1, 1, 1] },
    });
    expect(byTag(overlays, 'voice:r1')).toMatchObject({ kind: 'zone', color: '#ff00ff66' });
    expect(byTag(overlays, 'desk-draft')).toMatchObject({ kind: 'zone' });
    expect(byTag(overlays, 'room-selection')).toMatchObject({ kind: 'outline', boxes: [{ min: [0, 0, 0], max: [1, 1, 1] }] });
  });
});
