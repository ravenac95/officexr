// 0.5 m grid — four voxels per KayKit 2×2×2 block (was 2 before v4 migration)
export const VOXEL_SIZE = 0.5;

/** A character's height, the clearance the nav grid plans for. */
export const CHAR_HEIGHT_M = 1.8;

export type CameraMode =
  | 'first-person'
  | 'third-person'
  | 'fixed'
  | 'top-down'
  | 'free-fly';
export const CAMERA_MODES: readonly CameraMode[] = [
  'first-person',
  'third-person',
  'fixed',
  'top-down',
  'free-fly',
];
