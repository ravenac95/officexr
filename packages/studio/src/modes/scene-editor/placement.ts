import type { WorldObjects } from '@officexr/sdk';
import type { CatalogService, InstanceGeometryService } from '@officexr/world/app';
import { dropToSurface } from '../room/dropToSurface.ts';
import {
  newObjectShape,
  overlapsAnyInstance,
  type FootprintOf,
} from '../room/footprint.ts';
import {
  snapToGridOrStack,
  snapToNearestFace,
  snapToVoxel,
  type NearbyObjectInfo,
  type SnapHit,
} from '../room/roomSnap.ts';

type Vec3 = [number, number, number];

/** Pull radius (metres) for the flush face-snap. Generous enough that a cursor
 * floating ~1m off a large tile's corner still snaps; 2.5m fits 4m-cubes and
 * smaller without overcommitting on floor clicks 5m+ from a cube. */
const FACE_SNAP_PULL_RADIUS_M = 2.5;

export interface SnapContext {
  nearbyObjects: readonly NearbyObjectInfo[];
  geometry: InstanceGeometryService;
  voxelSize: number;
  /** Camera world-Y; biases the stack-above-or-below tie. */
  cameraY?: number;
}

/** Footprints (metres) of every placed object, as the face-snap consumes them. */
export function nearbyObjectsOf(
  instances: WorldObjects['instances'],
  footprintOf: FootprintOf,
  voxelSize: number,
): NearbyObjectInfo[] {
  return instances.map((inst) => {
    const { min, max } = footprintOf(inst.position, inst.kindId, inst.yawDeg);
    return {
      position: inst.position,
      aabb: {
        min: [min[0] * voxelSize, min[1] * voxelSize, min[2] * voxelSize] as const,
        max: [max[0] * voxelSize, max[1] * voxelSize, max[2] * voxelSize] as const,
      },
    };
  });
}

function targetStrideOf(hit: SnapHit, geometry: SnapContext['geometry']) {
  if (hit.kind !== 'cube') {
    return undefined;
  }
  const [x, y, z] = geometry.tileStep(hit.kindId, hit.yawDeg);
  return { x, y, z };
}

/**
 * Snap a hit to a voxel for origin placement (Add tool, Tile click 1 and
 * their ghosts): face-snap flush against a nearby object, else grid snap
 * to the placing kind's stride, stacking above/below on overlap.
 */
export function snapForPlacement(
  hit: SnapHit,
  kindId: string | null,
  yawDeg: number,
  ctx: SnapContext,
): Vec3 {
  const { nearbyObjects, geometry, voxelSize, cameraY = 0 } = ctx;
  const targetStride = targetStrideOf(hit, geometry);
  if (!kindId) {
    return snapToVoxel(hit, voxelSize, undefined, targetStride);
  }
  const shape = newObjectShape(geometry, kindId, yawDeg);
  // Snap from the raycast hit point, not the cube's anchor (which always
  // reads as its bottom and would make the dominant-axis pick always -Y).
  return (
    snapToNearestFace(hit.point, shape, nearbyObjects, voxelSize, FACE_SNAP_PULL_RADIUS_M, {
      cameraY,
    }) ?? snapToGridOrStack(hit, shape, nearbyObjects, voxelSize, targetStride, cameraY)
  );
}

export interface PlacementContext extends SnapContext {
  compiled: WorldObjects;
  catalog: Pick<CatalogService, 'getKind'>;
  footprintOf: FootprintOf;
}

/**
 * Where a click with the staged kind lands, or null when placement is
 * rejected: a gravity kind with no support below, or a footprint that
 * overlaps an existing instance.
 */
export function resolvePlacement(
  hit: SnapHit,
  kindId: string,
  yawDeg: number,
  ctx: PlacementContext,
): Vec3 | null {
  let voxel = snapForPlacement(hit, kindId, yawDeg, ctx);
  if (ctx.catalog.getKind(kindId)?.gravity) {
    const settled = dropToSurface(voxel, { kindId, yawDeg }, ctx.compiled, ctx.footprintOf);
    if (!settled) {
      return null;
    }
    voxel = settled;
  }
  const proposed = ctx.footprintOf(voxel, kindId, yawDeg);
  if (overlapsAnyInstance(proposed, ctx.compiled.instances, ctx.footprintOf)) {
    return null;
  }
  return voxel;
}
