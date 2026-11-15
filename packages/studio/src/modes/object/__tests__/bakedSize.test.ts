import { describe, expect, it } from 'vitest';
import { bakedSize } from '../bakedSize.ts';

const localAABB = { min: { x: -1, y: 0, z: -0.5 }, max: { x: 1, y: 3, z: 0.5 } };

describe('bakedSize', () => {
  it('is the baked bounds at the scale they were baked at', () => {
    expect(bakedSize({ localAABB, scale: 2, bakedScale: 2 })).toEqual({ width: 2, height: 3, depth: 1 });
  });

  it('follows a scale changed since the bake', () => {
    expect(bakedSize({ localAABB, scale: 3, bakedScale: 1.5 })).toEqual({ width: 4, height: 6, depth: 2 });
  });

  it('takes bounds baked before the bake recorded its scale as at the current scale', () => {
    expect(bakedSize({ localAABB, scale: 3 })).toEqual({ width: 2, height: 3, depth: 1 });
  });

  it('has nothing for a kind never baked', () => {
    expect(bakedSize({ scale: 1 })).toBeUndefined();
  });
});
