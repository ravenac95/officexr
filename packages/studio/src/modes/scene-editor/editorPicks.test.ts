import { describe, expect, it } from 'vitest';
import type { PickEvent, Plane, Vec3 } from '@officexr/render-wasm';
import type { ObjectInstance } from '@officexr/sdk';
import { editorPick, pointerProjector, roomCenter, type ScreenRay } from './editorPicks.ts';

/** A ray from `origin` along `dir`, intersected exactly. */
function ray(origin: Vec3, dir: Vec3): ScreenRay {
  return {
    origin: () => origin,
    intersect({ normal, constant }: Plane) {
      const denom = normal[0] * dir[0] + normal[1] * dir[1] + normal[2] * dir[2];
      const t = -(normal[0] * origin[0] + normal[1] * origin[1] + normal[2] * origin[2] + constant) / denom;
      return t > 0 ? [origin[0] + t * dir[0], origin[1] + t * dir[1], origin[2] + t * dir[2]] : null;
    },
  };
}

const VOXEL = 0.5;
const DOWN = ray([1.1, 10, 2.2], [0, -1, 0]);

const pick = (target: PickEvent['target'], point: Vec3 | null = null, normal: Vec3 | null = null): PickEvent => ({
  kind: 'click',
  target,
  point,
  normal,
  button: 0,
  modifiers: { shift: true, ctrl: false, alt: false, meta: true },
  screen: [0, 0],
});

const block: ObjectInstance = { id: 'i1', sourceCommandId: 'c1', kindId: 'k', position: [4, 0, 2] } as ObjectInstance;
const ctx = {
  instancesById: new Map([[block.id, block]]),
  footprintOf: () => ({ min: [4, 0, 2] as const, max: [8, 4, 6] as const }),
  buildPlaneY: 1,
};

describe('pointerProjector', () => {
  it('snaps the floor hit at a height to voxels', () => {
    expect(pointerProjector(DOWN, VOXEL).onFloor(1)).toEqual([2, 2, 4]);
  });

  it('measures the height above an origin on the plane facing the camera, in whole steps', () => {
    // From 10 m out along +z, looking level at 3.1 m.
    const level = ray([0, 3.1, 10], [0, 0, -1]);
    expect(pointerProjector(level, VOXEL).heightAbove([0, 0, 0], 4)).toEqual(8);
    expect(pointerProjector(level, VOXEL).heightAbove([0, 0, 0], 1)).toEqual(6);
  });

  it('has no height straight above the origin', () => {
    expect(pointerProjector(ray([0, 10, 0], [0, -1, 0]), VOXEL).heightAbove([0, 0, 0], 1)).toBeNull();
  });
});

describe('editorPick', () => {
  it('an object pick is a face hit on its footprint', () => {
    expect(editorPick(pick({ type: 'object', id: 'i1' }, [4, 1, 2], [0, 1, 0]), DOWN, ctx)).toEqual({
      target: { type: 'object', commandId: 'c1' },
      hit: { kind: 'cube', cubePosition: [4, 0, 2], yawDeg: undefined, faceNormal: [0, 1, 0], kindId: 'k', point: { x: 4, y: 1, z: 2 } },
      cameraY: 10,
      modKey: true,
      shift: true,
    });
  });

  it('a ground pick lands on the build plane under the pointer', () => {
    expect(editorPick(pick({ type: 'ground' }, [1.1, 0, 2.2]), DOWN, ctx).hit).toEqual({
      kind: 'floor',
      point: { x: 1.1, y: 1, z: 2.2 },
    });
  });

  it('a layout pick rests on an up-facing surface at its exact height', () => {
    const layout = { type: 'layout', name: 'l', roomInstanceId: '' } as const;
    expect(editorPick(pick(layout, [1, 0.15, 1], [0, 1, 0]), DOWN, ctx).hit).toEqual({
      kind: 'floor',
      point: { x: 1, y: 0.15, z: 1 },
      exactY: true,
    });
    expect(editorPick(pick(layout, [1, 1, 1], [1, 0, 0]), DOWN, ctx).hit).toEqual({ kind: 'floor', point: { x: 1, y: 1, z: 1 } });
  });

  it('players and overlays are not tool targets', () => {
    expect(editorPick(pick({ type: 'player', id: 'p' }), DOWN, ctx)).toMatchObject({ target: null, hit: null });
  });
});

describe('roomCenter', () => {
  it('looks at the middle of the instances, and above the origin for an empty room', () => {
    expect(roomCenter([], VOXEL)).toEqual([0, 1, 0]);
    expect(roomCenter([block, { ...block, position: [0, 2, -2] }], VOXEL)).toEqual([1, 0.75, 0]);
  });
});
