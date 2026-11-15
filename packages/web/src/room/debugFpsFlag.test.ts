import { describe, expect, it } from 'vitest';
import { debugFpsEnabled } from './debugFpsFlag';

function memoryStorage() {
  const items = new Map<string, string>();
  return {
    getItem: (k: string) => items.get(k) ?? null,
    setItem: (k: string, v: string) => void items.set(k, v),
    removeItem: (k: string) => void items.delete(k),
  };
}

describe('debugFpsEnabled', () => {
  it('turns on from the URL and stays on without it until turned off', () => {
    const storage = memoryStorage();
    expect(debugFpsEnabled('', storage)).toBe(false);
    expect(debugFpsEnabled('?enable_debug_fps=1', storage)).toBe(true);
    expect(debugFpsEnabled('?other=x', storage)).toBe(true);
    expect(debugFpsEnabled('?enable_debug_fps=0', storage)).toBe(false);
    expect(debugFpsEnabled('', storage)).toBe(false);
  });

  it('honours the URL when storage throws', () => {
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
      removeItem: () => {
        throw new Error('blocked');
      },
    };
    expect(debugFpsEnabled('?enable_debug_fps=1', blocked)).toBe(true);
    expect(debugFpsEnabled('', blocked)).toBe(false);
  });
});
