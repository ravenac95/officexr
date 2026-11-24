import type { Overlay } from '@officexr/render-wasm';
import { markerText, type MissionMarkerTarget } from '../renderer/missionMarker.ts';
import type { PlacementPreview } from './placementFrame.ts';

/** `MissionMarker.tsx`'s beacon stands on the office floor. */
const MISSION_FLOOR_Y_M = 1;
const MISSION_COLOUR = '#0a84ff';
const GHOST_OPACITY = 0.55;
const GHOST_VALID = '#2ee59d';
const GHOST_BLOCKED = '#ff4d4d';
const TARGET_COLOUR = '#ffd23fe6';
const GHOST_ID = '__ghost__';

/** The active mission's beacon, captioned with the whole metres to go once the player's position is known. */
export function missionBeacon(target: MissionMarkerTarget, self: { x: number; z: number } | null): Overlay {
  const metres = self ? Math.hypot(self.x - target.x, self.z - target.z) : null;
  return {
    tag: 'mission-marker',
    kind: 'beacon',
    at: [target.x, MISSION_FLOOR_Y_M, target.z],
    color: MISSION_COLOUR,
    label: markerText(target.label, metres),
    pickable: false,
  };
}

/**
 * `PlacementController.tsx`'s look for build mode's preview (`placementPreview`): a ghost of the
 * chosen prop where it would land, green where it fits and red where it doesn't, and an outline,
 * drawn through walls, round the placed prop the player points at.
 */
// ponytail: the tint replaces the prop's colours; upstream kept its texture and glowed green or red.
export function placementOverlays({ ghost, target }: PlacementPreview): Overlay[] {
  return [
    ...(ghost
      ? [
          {
            tag: 'placement-ghost',
            kind: 'ghost' as const,
            objects: [{ id: GHOST_ID, kindId: ghost.kindId, position: ghost.position, yawDeg: ghost.yawDeg }],
            opacity: GHOST_OPACITY,
            tint: ghost.valid ? GHOST_VALID : GHOST_BLOCKED,
            pickable: false,
          },
        ]
      : []),
    ...(target ? [{ tag: 'placement-target', kind: 'outline' as const, boxes: [target], color: TARGET_COLOUR }] : []),
  ];
}
