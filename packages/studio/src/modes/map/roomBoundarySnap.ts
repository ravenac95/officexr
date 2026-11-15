/**
 * Pure helper that snaps a moving room's voxel AABB to its neighbors'
 * voxel AABBs along the X and Z axes. Used by the Map editor's Move
 * tool so dragging a room near another room makes their faces (or
 * walls) align.
 *
 * No three.js, no react — keeps the math testable in isolation.
 */

/**
 * A room's voxel-space AABB in world coordinates (not room-local). Min
 * and max are inclusive voxel indices: a 4×4 room placed at world
 * voxel (0,0,0) covers x ∈ {0,1,2,3}, so `min=[0,0,0]`, `max=[3,_,3]`.
 */
export interface RoomAABBVoxel {
  id: string;
  min: [number, number, number];
  max: [number, number, number];
}

/**
 * Snap result: per-axis voxel delta to add to the moving room's
 * proposed position. 0 means "no snap on this axis"; the caller falls
 * back to the proposed (grid-snapped) position.
 */
export interface SnapDelta {
  dx: number;
  dz: number;
}

/**
 * For each X/Z axis, consider candidate snaps that align the moving
 * room's faces (or walls) with a neighbor:
 *
 *   - **face-to-face**: `moving.min == neighbor.max + 1` (moving
 *     sits flush to the right of neighbor) and the mirror.
 *   - **co-planar**: `moving.min == neighbor.min` (rooms start on the
 *     same line) and `moving.max == neighbor.max` (rooms end on the
 *     same line). Lets two rooms line up along a shared long wall.
 *
 * Picks the candidate with smallest |delta| within `thresholdVoxels`.
 * Ties prefer the smaller |delta| then the lexicographically smaller
 * neighbor id (stable result regardless of array order).
 *
 * Returns `{ dx: 0, dz: 0 }` if no candidate qualifies — caller keeps
 * the grid-snapped position.
 */
export function snapRoomToNeighbors(
  moving: RoomAABBVoxel,
  neighbors: readonly RoomAABBVoxel[],
  thresholdVoxels: number,
): SnapDelta {
  return {
    dx: snapAxis(moving, neighbors, thresholdVoxels, 0),
    dz: snapAxis(moving, neighbors, thresholdVoxels, 2),
  };
}

function snapAxis(
  moving: RoomAABBVoxel,
  neighbors: readonly RoomAABBVoxel[],
  threshold: number,
  axis: 0 | 2,
): number {
  // A snap candidate is a (delta, neighborId) pair. We collect all
  // candidates that bring some moving face/wall into alignment with
  // some neighbor face/wall, then pick the smallest qualifying |delta|.
  interface Candidate {
    delta: number;
    neighborId: string;
  }
  const candidates: Candidate[] = [];
  const mMin = moving.min[axis];
  const mMax = moving.max[axis];

  for (const n of neighbors) {
    if (n.id === moving.id) continue;
    const nMin = n.min[axis];
    const nMax = n.max[axis];

    // Face-to-face: moving sits flush against neighbor.
    //   moving.min == neighbor.max + 1  → delta = (nMax + 1) - mMin
    //   moving.max == neighbor.min - 1  → delta = (nMin - 1) - mMax
    candidates.push({ delta: nMax + 1 - mMin, neighborId: n.id });
    candidates.push({ delta: nMin - 1 - mMax, neighborId: n.id });

    // Co-planar: rooms line up along a shared wall on this axis.
    //   moving.min == neighbor.min      → delta = nMin - mMin
    //   moving.max == neighbor.max      → delta = nMax - mMax
    candidates.push({ delta: nMin - mMin, neighborId: n.id });
    candidates.push({ delta: nMax - mMax, neighborId: n.id });
  }

  let best: Candidate | null = null;
  for (const c of candidates) {
    if (Math.abs(c.delta) > threshold) continue;
    if (c.delta === 0) {
      // Already aligned — nothing to do. We keep iterating so a
      // tied non-zero delta on a different neighbor doesn't win.
      return 0;
    }
    if (best === null) {
      best = c;
      continue;
    }
    const cAbs = Math.abs(c.delta);
    const bAbs = Math.abs(best.delta);
    if (cAbs < bAbs) {
      best = c;
    } else if (cAbs === bAbs && c.neighborId < best.neighborId) {
      best = c;
    }
  }
  return best?.delta ?? 0;
}

/**
 * Where a Move drag puts a room: the floor `point` under the pointer, less the press's `offset` from
 * the room's anchor, snapped to the voxel grid and then to a neighbour within `thresholdVoxels`.
 * `self` is the room's AABB at `current`. Null when the room stays put.
 */
export function dragRoomTo(
  point: { x: number; z: number },
  offset: { x: number; z: number },
  current: readonly [number, number, number],
  self: RoomAABBVoxel | null,
  neighbors: readonly RoomAABBVoxel[],
  voxelSize: number,
  thresholdVoxels: number,
): [number, number, number] | null {
  let vx = Math.round((point.x - offset.x) / voxelSize);
  let vz = Math.round((point.z - offset.z) / voxelSize);
  if (self && neighbors.length > 0) {
    const tx = vx - current[0];
    const tz = vz - current[2];
    const proposed: RoomAABBVoxel = {
      id: self.id,
      min: [self.min[0] + tx, self.min[1], self.min[2] + tz],
      max: [self.max[0] + tx, self.max[1], self.max[2] + tz],
    };
    const { dx, dz } = snapRoomToNeighbors(proposed, neighbors, thresholdVoxels);
    vx += dx;
    vz += dz;
  }
  return vx === current[0] && vz === current[2] ? null : [vx, current[1], vz];
}
