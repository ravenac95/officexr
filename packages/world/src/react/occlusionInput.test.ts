import { describe, expect, it } from 'vitest';
import type { PlayerState } from '@officexr/sdk';
import type { WaterVolume } from '../motion/water.ts';
import { occlusionCutsAway, occlusionFades, occlusionInput } from './occlusionInput.ts';

const R = 0.4;
const player = (id: string, x: number, more: Partial<PlayerState> = {}) =>
  ({ id, pos: { x, y: 1, z: 0 }, ...more }) as PlayerState;
const POOL: WaterVolume = { id: 'pool', minX: 10, maxX: 14, minZ: -2, maxZ: 2, baseY: 0, surfaceY: 0.4 };

describe('occlusionInput', () => {
  it("aims at the player's head and chest and cuts the roof only when asked", () => {
    const players = { me: player('me', 0) };
    const input = occlusionInput(players, 'me', [], R, true)!;
    // Feet at body root 1 + 0.9 - 0.4.
    expect(input.targets).toEqual([
      [0, 1.5 + 1.55, 0],
      [0, 1.5 + 1, 0],
    ]);
    expect(input.cutaway).toEqual([0, 1.5, 0]);
    expect(input.ignore).toEqual([]);
    expect(input.anchor).toBeNull();
    expect(input.occupants).toEqual([]);
    expect(occlusionInput(players, 'me', [], R, false)!.cutaway).toBeNull();
    expect(occlusionInput(players, 'gone', [], R, true)).toBeNull();
  });

  it('leaves out the seat, ridden rack or pool each player is in, and lists them as occupants', () => {
    const players = {
      me: player('me', 0, { seat: 'sofa#2' }),
      rider: player('rider', 5, { pose: 'scooter', poseRack: 'rack' }),
      swimmer: player('swimmer', 12, { pose: 'swim' }),
      walker: player('walker', 20),
    };
    const input = occlusionInput(players, 'me', [POOL], R, false)!;
    expect(input.ignore).toEqual(['sofa']);
    expect(input.anchor).toEqual([0, 1, 0]);
    expect(input.occupants?.map((o) => [o.id, o.ignore])).toEqual([
      ['me', ['sofa']],
      ['rider', ['rack']],
      ['swimmer', ['pool']],
    ]);
  });

  it('fades for the cameras that look at the player from outside, and cuts from above', () => {
    expect(occlusionFades('fixed')).toBe(true);
    expect(occlusionFades('third-person')).toBe(true);
    expect(occlusionFades('first-person')).toBe(false);
    expect(occlusionCutsAway('third-person')).toBe(false);
    expect(occlusionCutsAway('fixed')).toBe(true);
  });
});
