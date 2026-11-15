import type { JoystickKind } from '@officexr/render-wasm';

const STORAGE_KEY = 'officexr:joystickKind';

/** The touch joystick the user last picked, else Bevy's own. */
export function storedJoystickKind(storage: Pick<Storage, 'getItem'> = localStorage): JoystickKind {
  try {
    return storage.getItem(STORAGE_KEY) === 'dom' ? 'dom' : 'bevy';
  } catch {
    return 'bevy';
  }
}

export function storeJoystickKind(kind: JoystickKind, storage: Pick<Storage, 'setItem'> = localStorage): void {
  try {
    storage.setItem(STORAGE_KEY, kind);
  } catch {
    // Unremembered choice: the reload uses the default joystick.
  }
}
