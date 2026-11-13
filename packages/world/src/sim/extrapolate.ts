import type { Vec3 } from '@officexr/sdk';

/** If updates stop arriving, the projected position freezes at this age instead of running away with stale velocity. */
export const EXTRAPOLATION_CAP_S = 0.1;

export interface Extrapolatable {
  pos: Vec3;
  vel: Vec3;
  /** Receive time of `pos`; absent for the local player, whose position is fresh every frame. */
  tRecv?: number;
}

/** `player.pos` advanced by its broadcast velocity since `tRecv`, capped at `EXTRAPOLATION_CAP_S`. */
export function extrapolatePos(player: Extrapolatable, nowMs: number): Vec3 {
  if (player.tRecv === undefined) {
    return { x: player.pos.x, y: player.pos.y, z: player.pos.z };
  }
  const elapsed = Math.min(EXTRAPOLATION_CAP_S, (nowMs - player.tRecv) / 1000);
  return {
    x: player.pos.x + player.vel.x * elapsed,
    y: player.pos.y + player.vel.y * elapsed,
    z: player.pos.z + player.vel.z * elapsed,
  };
}
