import type { WorldObjectKind } from '@officexr/world';

export type KindSize = NonNullable<WorldObjectKind['dimensions']>;

/**
 * The size of the bounds the dimensions bake measured, at the kind's current `scale`: the bake
 * measures at the scale it records as `bakedScale`. `undefined` for a kind never baked.
 */
export function bakedSize(kind: Pick<WorldObjectKind, 'localAABB' | 'scale' | 'bakedScale'>): KindSize | undefined {
  if (!kind.localAABB) {
    return undefined;
  }
  const { min, max } = kind.localAABB;
  const rescale = kind.scale / (kind.bakedScale ?? kind.scale);
  return { width: (max.x - min.x) * rescale, height: (max.y - min.y) * rescale, depth: (max.z - min.z) * rescale };
}
