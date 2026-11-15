import { useEffect, useEffectEvent, useState, type RefObject } from 'react';
import type { RendererModule, ViewId } from '@officexr/render-wasm';
import { bootBevy, playCanvas } from './bevyRenderer.ts';

export interface BevyView {
  wasm: RendererModule;
  view: ViewId;
}

interface BevySentinels {
  /** The mounted view has loaded every model; for Playwright. */
  __officexrBevyReady?: boolean;
}

/** Sets `window.__officexrBevyReady`, which the page's mounted view clears again on unmount. */
export function setBevyReady(ready: boolean): void {
  (window as BevySentinels).__officexrBevyReady = ready;
}

/**
 * A world view on the page's primary canvas, mounted into `container` for the component's lifetime:
 * null until Bevy has booted. A failed boot goes to `onError`.
 */
export function usePrimaryBevyView(
  container: RefObject<HTMLElement | null>,
  onError: (error: Error) => void,
): BevyView | null {
  const [bevy, setBevy] = useState<BevyView | null>(null);
  const reportError = useEffectEvent(onError);
  useEffect(() => {
    const canvas = playCanvas();
    container.current?.append(canvas);
    let cancelled = false;
    let mounted: BevyView | null = null;
    bootBevy(canvas)
      .then((wasm) => {
        if (!cancelled) {
          mounted = { wasm, view: wasm.createView(canvas, 'world') };
          setBevy(mounted);
        }
      })
      .catch((error: Error) => reportError(error));
    return () => {
      cancelled = true;
      if (mounted) {
        mounted.wasm.destroyView(mounted.view);
      }
      canvas.remove();
      delete (window as BevySentinels).__officexrBevyReady;
    };
  }, [container]);
  return bevy;
}
