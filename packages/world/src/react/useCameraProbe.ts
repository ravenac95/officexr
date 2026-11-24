import { useEffect, type RefObject } from 'react';
import type { Store } from '@officexr/sdk';
import { feetPosition } from '../physics/rules.ts';
import { CHAR_HEIGHT_M } from '../renderer/config.ts';
import type { BevyView } from './usePrimaryBevyView.ts';

export interface CameraProbe {
  /** The local player's chest in normalised device coordinates of the view's camera, or null before it has drawn. */
  selfNdc: () => { x: number; y: number } | null;
}

/** `ndc` of a point `screen` CSS px into a `width` × `height` canvas: x right, y up, ±1 at the edges. */
export function screenToNdc(screen: readonly [number, number], width: number, height: number): { x: number; y: number } {
  return { x: (screen[0] / width) * 2 - 1, y: 1 - (screen[1] / height) * 2 };
}

/** Publishes `window.__OFFICE_CAMERA__` for the camera e2e specs (the player stays centred in Map and Angled). */
export function useCameraProbe(
  bevy: BevyView | null,
  container: RefObject<HTMLElement | null>,
  store: Store,
  selfId: string,
): void {
  useEffect(() => {
    if (!bevy) {
      return;
    }
    const probe: CameraProbe = {
      selfNdc: () => {
        const self = store.getState().players[selfId];
        const box = container.current;
        if (!self || !box || box.clientWidth === 0 || box.clientHeight === 0) {
          return null;
        }
        const feet = feetPosition(self.pos, store.getState().worldSettings.charRadius);
        try {
          const at = bevy.wasm.projectToScreen(bevy.view, [feet.x, feet.y + CHAR_HEIGHT_M / 2, feet.z]);
          return at && screenToNdc(at, box.clientWidth, box.clientHeight);
        } catch {
          return null;
        }
      },
    };
    const target = globalThis as unknown as { __OFFICE_CAMERA__?: CameraProbe };
    target.__OFFICE_CAMERA__ = probe;
    return () => {
      delete target.__OFFICE_CAMERA__;
    };
  }, [bevy, container, store, selfId]);
}
