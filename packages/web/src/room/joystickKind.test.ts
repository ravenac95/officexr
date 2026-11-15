import { describe, expect, it } from 'vitest';
import { storedJoystickKind, storeJoystickKind } from './joystickKind';

function memoryStorage() {
  const data: Record<string, string> = {};
  return { getItem: (k: string) => data[k] ?? null, setItem: (k: string, v: string) => void (data[k] = v) };
}

describe('joystick kind', () => {
  it("defaults to Bevy's joystick and remembers a switch to the DOM one and back", () => {
    const storage = memoryStorage();
    expect(storedJoystickKind(storage)).toBe('bevy');
    storeJoystickKind('dom', storage);
    expect(storedJoystickKind(storage)).toBe('dom');
    storeJoystickKind('bevy', storage);
    expect(storedJoystickKind(storage)).toBe('bevy');
  });

  it('falls back to the default when storage throws', () => {
    const blocked = () => {
      throw new Error('blocked');
    };
    expect(storedJoystickKind({ getItem: blocked })).toBe('bevy');
    expect(() => storeJoystickKind('dom', { setItem: blocked })).not.toThrow();
  });
});
