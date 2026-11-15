import { describe, expect, it } from 'vitest';
import { MUGSHOT_PITCH_DEG, mugshotCamera, mugshotWorld } from '../mugshotScene.ts';

const FIXED = { distanceM: 6, height: 1.7 };

describe('mugshotCamera', () => {
  it.each([
    [0, [0, 1.7, -6]],
    [90, [6, 1.7, 0]],
    [180, [0, 1.7, 6]],
    [270, [-6, 1.7, 0]],
  ] as const)('stands %i° round the origin at the fixed distance and height', (azimuthDeg, position) => {
    const pose = mugshotCamera({ azimuthDeg, ...FIXED });
    pose.position.forEach((v, i) => expect(v).toBeCloseTo(position[i], 9));
  });

  it('looks toward the origin, pitched down by the fixed pitch', () => {
    const { position, lookAt } = mugshotCamera({ azimuthDeg: 90, ...FIXED });
    const dir = lookAt.map((v, i) => v - position[i]);
    expect(Math.hypot(...dir)).toBeCloseTo(1, 9);
    expect(dir[0]).toBeLessThan(0);
    expect(dir[2]).toBeCloseTo(0, 9);
    expect((Math.asin(dir[1]) * 180) / Math.PI).toBeCloseTo(MUGSHOT_PITCH_DEG, 9);
  });
});

describe('mugshotWorld', () => {
  it('replaces the scene with the cube kinds the mode uses, from the catalog', () => {
    const kinds = [{ id: 'stone' }, { id: 'colored_block_blue' }, { id: 'chair' }];
    const world = mugshotWorld(kinds, 'gltf', {} as never, {} as never);
    expect(world.replace).toBe(true);
    expect(world.kinds).toEqual([{ id: 'stone' }, { id: 'colored_block_blue' }]);
    expect(world.objects?.instances).toHaveLength(4);
    expect(mugshotWorld(kinds, 'primitive', {} as never, {} as never).kinds).toEqual([]);
  });
});
