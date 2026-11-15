import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import type { CameraPose, WorldDiff } from '@officexr/render-wasm';
import type { BackgroundViewConfig, LightingViewConfig } from '@officexr/world/app';

export type CubeMode = 'gltf' | 'primitive';

/** The voxel size the mugshot cubes are laid out in; it must be the global `VOXEL_SIZE`. */
const CUBE_SIZE = 0.5;
const NEAR = 0.1;
const FAR = 2000;

/** The mugshot camera's fixed pitch (degrees) and vertical field of view. */
export const MUGSHOT_PITCH_DEG = -8;
export const MUGSHOT_FOV = 40;

/**
 * A 2×2 square of 2 m cubes centred on the origin, tops at y = 1: voxels -4..0 on X/Z at 0.5 m.
 * Both modes share instance ids, so the colliders are identical and only the drawn mesh differs.
 */
const GLTF_CUBES = [
  { id: 'm-0-0', sourceCommandId: 'mugshot', kindId: 'colored_block_blue', position: [-4, -2, -4] },
  { id: 'm-1-0', sourceCommandId: 'mugshot', kindId: 'stone', position: [0, -2, -4] },
  { id: 'm-0-1', sourceCommandId: 'mugshot', kindId: 'stone', position: [-4, -2, 0] },
  { id: 'm-1-1', sourceCommandId: 'mugshot', kindId: 'colored_block_blue', position: [0, -2, 0] },
] as const;

const PRIMITIVE_CUBES = [
  { id: 'm-0-0', sourceCommandId: 'mugshot', kindId: '__primitive_blue', position: [-4, -2, -4] },
  { id: 'm-1-0', sourceCommandId: 'mugshot', kindId: '__primitive_stone', position: [0, -2, -4] },
  { id: 'm-0-1', sourceCommandId: 'mugshot', kindId: '__primitive_stone', position: [-4, -2, 0] },
  { id: 'm-1-1', sourceCommandId: 'mugshot', kindId: '__primitive_blue', position: [0, -2, 0] },
] as const;

export function cubesForMode(mode: CubeMode) {
  return (mode === 'gltf' ? GLTF_CUBES : PRIMITIVE_CUBES).map((c) => ({
    ...c,
    position: [...c.position] as [number, number, number],
  }));
}

/** The studio's default lighting without the sun disc, which would dominate a pixel diff. */
export const DEFAULT_MUGSHOT_LIGHTING: LightingViewConfig = {
  ...DEFAULT_WORLD_CONSTANTS.lighting,
  showSunDisc: false,
};

export const DEFAULT_MUGSHOT_BACKGROUND: BackgroundViewConfig = DEFAULT_WORLD_CONSTANTS.background;

/**
 * The whole mugshot scene as one `setWorld` diff. `catalogKinds` must be the served catalog's,
 * since the character settles on the kinds' colliders.
 */
export function mugshotWorld(
  catalogKinds: readonly { id: string }[],
  mode: CubeMode,
  lighting: LightingViewConfig,
  background: BackgroundViewConfig,
): WorldDiff {
  const instances = cubesForMode(mode);
  const kindIds = new Set<string>(instances.map((i) => i.kindId));
  return {
    replace: true,
    kinds: catalogKinds.filter((k) => kindIds.has(k.id)),
    objects: { cubeSize: CUBE_SIZE, instances },
    lighting,
    background,
  };
}

/**
 * The fixed camera `distanceM` from the origin toward `azimuthDeg` (0 looks from -Z, 180 from +Z),
 * `height` up, pitched `MUGSHOT_PITCH_DEG`: the fixed camera mode's leash around `lookAt: [0, 0, 0]`.
 */
export function mugshotCamera({ azimuthDeg, distanceM, height }: { azimuthDeg: number; distanceM: number; height: number }): CameraPose {
  const az = (azimuthDeg * Math.PI) / 180;
  const pitch = (MUGSHOT_PITCH_DEG * Math.PI) / 180;
  const position: [number, number, number] = [Math.sin(az) * distanceM, height, -Math.cos(az) * distanceM];
  return {
    position,
    lookAt: [
      position[0] - Math.sin(az) * Math.cos(pitch),
      position[1] + Math.sin(pitch),
      position[2] + Math.cos(az) * Math.cos(pitch),
    ],
    fovDeg: MUGSHOT_FOV,
    near: NEAR,
    far: FAR,
  };
}
