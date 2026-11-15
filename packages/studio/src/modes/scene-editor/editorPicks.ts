import type { PickEvent, Plane, Vec3 as RenderVec3 } from '@officexr/render-wasm';
import type { ObjectInstance } from '@officexr/sdk';
import type { FootprintOf } from '../room/footprint.ts';
import type { SnapHit } from '../room/roomSnap.ts';

type Vec3 = [number, number, number];

/** The pointer projected onto the editor's construction planes, in voxels. */
export interface PointerProjector {
  /** The voxel under the pointer on the horizontal plane at world height `planeY` (m). */
  onFloor(planeY: number): Vec3 | null;
  /**
   * The pointer's height above `origin` in voxels, snapped to whole `stepY`s, on the vertical plane
   * through `origin` that faces the camera.
   */
  heightAbove(origin: Vec3, stepY: number): number | null;
}

/** What a pointer is over, as the tools see it: an object, the floor (ground or a baked layout), or neither. */
export type EditorTarget = { type: 'object'; commandId: string } | { type: 'floor' } | null;

export interface EditorPick {
  target: EditorTarget;
  /** Where on the target, for the placing tools; null off a target, or on a face with no normal. */
  hit: SnapHit | null;
  /** The camera's world Y: stack ties resolve toward the viewer. */
  cameraY: number;
  modKey: boolean;
  shift: boolean;
}

/** The view's camera rays, as the renderer's `rayAtScreen` / `intersectPlane` give them for one screen point. */
export interface ScreenRay {
  origin(): RenderVec3;
  intersect(plane: Plane): RenderVec3 | null;
}

const horizontal = (y: number): Plane => ({ normal: [0, 1, 0], constant: -y });

/** An up-facing hit (cos ≈ 45°) is a surface to rest on at its exact height. */
const UP_FACING_MIN_Y = 0.7;

export function pointerProjector(ray: ScreenRay, voxelSize: number): PointerProjector {
  return {
    onFloor(planeY) {
      const hit = ray.intersect(horizontal(planeY));
      return hit && [Math.round(hit[0] / voxelSize), Math.round(planeY / voxelSize), Math.round(hit[2] / voxelSize)];
    },
    heightAbove(origin, stepY) {
      const o = origin.map((v) => v * voxelSize);
      const camera = ray.origin();
      const len = Math.hypot(camera[0] - o[0], camera[2] - o[2]);
      // Straight above or below the origin the facing plane is undefined.
      if (len < 1e-3) return null;
      const normal: RenderVec3 = [(camera[0] - o[0]) / len, 0, (camera[2] - o[2]) / len];
      const hit = ray.intersect({ normal, constant: -(normal[0] * o[0] + normal[2] * o[2]) });
      if (!hit) return null;
      const step = Math.max(1, stepY);
      return Math.round((hit[1] - o[1]) / (step * voxelSize)) * step;
    },
  };
}

export interface PickContext {
  instancesById: ReadonlyMap<string, ObjectInstance>;
  footprintOf: FootprintOf;
  /** World height (m) of the build plane a ground pick lands on. */
  buildPlaneY: number;
}

/** A Bevy pick as the tools take it. */
export function editorPick(e: PickEvent, ray: ScreenRay, ctx: PickContext): EditorPick {
  const base = {
    cameraY: ray.origin()[1],
    modKey: e.modifiers.ctrl || e.modifiers.meta,
    shift: e.modifiers.shift,
  };
  const { target, point, normal } = e;
  if (target.type === 'object') {
    const inst = ctx.instancesById.get(target.id);
    if (!inst) return { ...base, target: null, hit: null };
    const hit: SnapHit | null =
      point && normal
        ? {
            kind: 'cube',
            cubePosition: ctx.footprintOf(inst.position, inst.kindId, inst.yawDeg).min,
            yawDeg: inst.yawDeg,
            faceNormal: normal,
            kindId: inst.kindId,
            point: { x: point[0], y: point[1], z: point[2] },
          }
        : null;
    return { ...base, target: { type: 'object', commandId: inst.sourceCommandId }, hit };
  }
  if (target.type === 'layout') {
    const hit: SnapHit | null = point && {
      kind: 'floor',
      point: { x: point[0], y: point[1], z: point[2] },
      ...((normal?.[1] ?? 0) >= UP_FACING_MIN_Y && { exactY: true }),
    };
    return { ...base, target: { type: 'floor' }, hit };
  }
  if (target.type === 'ground') {
    const p = ray.intersect(horizontal(ctx.buildPlaneY)) ?? point;
    const hit: SnapHit | null = p && { kind: 'floor', point: { x: p[0], y: ctx.buildPlaneY, z: p[2] } };
    return { ...base, target: { type: 'floor' }, hit };
  }
  return { ...base, target: null, hit: null };
}

/** Where the orbit camera looks: the middle of the room's instances. */
export function roomCenter(instances: readonly ObjectInstance[], voxelSize: number): Vec3 {
  if (instances.length === 0) return [0, 1, 0];
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const { position } of instances) {
    const p = [position[0], position[1] + 0.5, position[2]];
    for (let i = 0; i < 3; i++) {
      min[i] = Math.min(min[i], p[i] * voxelSize);
      max[i] = Math.max(max[i], p[i] * voxelSize);
    }
  }
  return [0, 1, 2].map((i) => (min[i] + max[i]) / 2) as Vec3;
}
