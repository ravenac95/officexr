import { describe, expect, it } from 'vitest';
import { twoStopHorizonColor } from './horizonColor.ts';

describe('twoStopHorizonColor', () => {
  it('mixes 7.41% of the way from the bottom colour to the top in linear light', () => {
    // linear 0.0741 is sRGB ≈ 0.303, so 77 = 0x4d.
    expect(twoStopHorizonColor('#ffffff', '#000000')).toBe('#4d4d4d');
  });

  it('keeps a uniform sky unchanged', () => {
    expect(twoStopHorizonColor('#87ceeb', '#87ceeb')).toBe('#87ceeb');
  });
});
