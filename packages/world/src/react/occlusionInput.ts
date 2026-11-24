import type { PlayerState } from '@officexr/sdk';
import type { OcclusionInput, Vec3 as Xyz } from '@officexr/render-wasm';
import type { WaterVolume } from '../motion/water.ts';
import { feetPosition } from '../physics/rules.ts';
import type { CameraMode } from '../renderer/config.ts';
import { createAnchor } from '../renderer/occlusion/occlusionMath.ts';
import { collectOccupiedAnchor } from '../renderer/occlusion/occupiedAnchor.ts';
import { collectOccupiedInstanceIds } from '../renderer/occlusion/occupiedInstances.ts';

const HEAD_HEIGHT = 1.55;
const CHEST_HEIGHT = 1.0;

/** Fading follows a camera that looks at the player from outside: the fixed and third-person ones. */
export const occlusionFades = (mode: CameraMode) => mode === 'fixed' || mode === 'third-person';

/** The roof over the player is cut away only from above. */
export const occlusionCutsAway = (mode: CameraMode) => mode === 'fixed';

function targets(feet: { x: number; y: number; z: number }): Xyz[] {
  return [
    [feet.x, feet.y + HEAD_HEIGHT, feet.z],
    [feet.x, feet.y + CHEST_HEIGHT, feet.z],
  ];
}

/** The seat, ridden rack or pool a player occupies, and the point it occupies, as `setOcclusion` takes them. */
function occupied(player: PlayerState | undefined, water: readonly WaterVolume[], charRadius: number) {
  const ignore = [...collectOccupiedInstanceIds(new Set(), player, water)];
  const a = collectOccupiedAnchor(createAnchor(), player, charRadius);
  const anchor: Xyz | null = a.active ? [a.x, a.y, a.z] : null;
  return { ignore, anchor };
}

/**
 * What the play camera must keep in sight this frame: the local player's head and chest, past
 * whatever it occupies; with `cutaway`, the roof over its feet. Every other player in a seat, ride
 * or pool rides along as an occupant, so the view can tell whether only that hides them.
 */
export function occlusionInput(
  players: Readonly<Record<string, PlayerState>>,
  selfId: string,
  water: readonly WaterVolume[],
  charRadius: number,
  cutaway: boolean,
): OcclusionInput | null {
  const self = players[selfId];
  if (!self) {
    return null;
  }
  const feet = feetPosition(self.pos, charRadius);
  const occupants = Object.values(players).flatMap((p) => {
    const own = occupied(p, water, charRadius);
    if (own.ignore.length === 0 && own.anchor === null) {
      return [];
    }
    return [{ id: p.id, targets: targets(feetPosition(p.pos, charRadius)), ...own }];
  });
  return {
    targets: targets(feet),
    ...occupied(self, water, charRadius),
    cutaway: cutaway ? [feet.x, feet.y, feet.z] : null,
    occupants,
  };
}
