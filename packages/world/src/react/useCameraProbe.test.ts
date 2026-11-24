import { describe, expect, it } from 'vitest';
import { screenToNdc } from './useCameraProbe.ts';

describe('screenToNdc', () => {
  it('maps the canvas centre to 0 and its corners to ±1, y up', () => {
    expect(screenToNdc([400, 300], 800, 600)).toEqual({ x: 0, y: 0 });
    expect(screenToNdc([0, 0], 800, 600)).toEqual({ x: -1, y: 1 });
    expect(screenToNdc([800, 600], 800, 600)).toEqual({ x: 1, y: -1 });
  });
});
