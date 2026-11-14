import { describe, expect, it, vi } from 'vitest';
import { sitOnPress, type SeatingSource } from './seated.ts';

describe('sitOnPress', () => {
  it('sits only on the frame E goes down', () => {
    const seating = { sit: vi.fn(), stand: vi.fn(), setStandingCheck: vi.fn() } satisfies SeatingSource;
    const keys = new Set(['e']);
    expect(sitOnPress(keys, false, seating)).toBe(true);
    expect(sitOnPress(keys, true, seating)).toBe(true);
    expect(sitOnPress(new Set(), true, seating)).toBe(false);
    expect(seating.sit).toHaveBeenCalledTimes(1);
  });
});
