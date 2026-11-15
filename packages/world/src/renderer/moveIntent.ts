import type { Vec3 } from '@officexr/sdk';

/** Analog walking intent: `x` is strafe (right +), `z` is forward (+), within the unit circle. */
export type MoveInput = { x: number; z: number };

export const NO_MOVE: Readonly<MoveInput> = Object.freeze({ x: 0, z: 0 });

/** A movement source that steers in world space, used when neither keys nor the stick give input. */
export interface AutoWalkSource {
  /** World-space unit XZ direction from the self body's position, or null to stand still. */
  intent(selfPos: Vec3): { x: number; z: number } | null;
  /** Called on frames with real movement input. */
  cancel(): void;
}
