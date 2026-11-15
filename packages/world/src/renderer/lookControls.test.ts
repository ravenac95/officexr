import { describe, expect, it } from 'vitest';
import { wrapAngle, yawFromDirection } from './lookControls.ts';

describe('yawFromDirection', () => {
  it.each([0, 0.7, -2.4, Math.PI / 2])('inverts the yaw convention at yaw %s, whatever the pitch', (yaw) => {
    for (const pitch of [0, -0.6, 1.1]) {
      const dir = { x: -Math.sin(yaw) * Math.cos(pitch), z: -Math.cos(yaw) * Math.cos(pitch) };
      expect(yawFromDirection(dir)).toBeCloseTo(yaw);
    }
  });
});

describe('wrapAngle', () => {
  it.each([
    ['-π to π', -Math.PI, Math.PI],
    ['3π to π', 3 * Math.PI, Math.PI],
    ['-3π to π', -3 * Math.PI, Math.PI],
    ['an in-range value unchanged', 1.25, 1.25],
    ['just past π to just past -π', Math.PI + 0.1, -Math.PI + 0.1],
  ])('maps %s', (_name, input, expected) => {
    expect(wrapAngle(input)).toBeCloseTo(expected, 9);
  });
});
