const STORAGE_KEY = 'officexr:debugFps';

/**
 * Whether the FPS overlay is on: `?enable_debug_fps=1` turns it on and remembers it in `storage`, so it
 * survives navigating into a room; `?enable_debug_fps=0` forgets it. Else the remembered choice.
 */
export function debugFpsEnabled(search: string, storage?: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>): boolean {
  const asked = new URLSearchParams(search).get('enable_debug_fps');
  try {
    if (asked === '1') {
      storage?.setItem(STORAGE_KEY, '1');
      return true;
    }
    if (asked === '0') {
      storage?.removeItem(STORAGE_KEY);
      return false;
    }
    return storage?.getItem(STORAGE_KEY) === '1';
  } catch {
    // Storage blocked (private mode): only the URL counts.
    return asked === '1';
  }
}
