import type { CameraMode } from './config.ts';

/** Yaw (forward = `-sin, -cos`; positive turns counter-clockwise from above) of a look direction, ignoring its pitch. */
export function yawFromDirection(dir: { x: number; z: number }): number {
  return Math.atan2(-dir.x, -dir.z);
}

/** Wraps an angle in radians into (-π, π]. */
export function wrapAngle(rad: number): number {
  const a = rad % (2 * Math.PI);
  return a <= -Math.PI ? a + 2 * Math.PI : a > Math.PI ? a - 2 * Math.PI : a;
}

/** Modes whose yaw and pitch the mouse turns, as the controls' `is_yaw_pitch_mode`. */
export function isYawPitchMode(mode: CameraMode): boolean {
  return mode === 'first-person' || mode === 'third-person';
}
