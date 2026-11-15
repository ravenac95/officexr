import React, { useEffect, useRef } from 'react';
import type { PickEvent, RendererModule, ViewId, ViewKind } from '@officexr/render-wasm';
import { bootBevy } from '@officexr/world/react';

export interface BevySentinels {
  /** The wasm module, for drivers to call it directly. */
  __officexrBevy?: RendererModule;
  /** The page's view ids, in canvas order. */
  __officexrBevyViews?: ViewId[];
  /** Every view has loaded and settled. */
  __officexrBevyReady?: boolean;
  /** Where the mugshot character's feet settled, in metres. */
  __officexrBevyMugshotFeetY?: number;
  /** With `?op=bevy&pick=1`, the world view's pick events so far. */
  __officexrBevyPicks?: PickEvent[];
}

export const bevySentinels = () => window as unknown as BevySentinels;

const canvasStyle: React.CSSProperties = { display: 'block', width: '100%', height: '100%' };

/** A Bevy view on a canvas for the component's lifetime; `onView` feeds it. */
export function BevyCanvas({
  id,
  kind,
  primary = false,
  onView,
}: {
  id?: string;
  kind: ViewKind;
  /** Draws into Bevy's primary window when Bevy boots here; mount it before the page's other canvases. */
  primary?: boolean;
  onView: (wasm: RendererModule, view: ViewId) => (() => void) | void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | void;
    let view: ViewId | null = null;
    void bootBevy(primary && canvas.current ? canvas.current : undefined).then((wasm) => {
      if (cancelled || !canvas.current) {
        return;
      }
      view = wasm.createView(canvas.current, kind);
      const s = bevySentinels();
      s.__officexrBevy = wasm;
      s.__officexrBevyViews = [...(s.__officexrBevyViews ?? []), view];
      cleanup = onView(wasm, view);
    });
    return () => {
      cancelled = true;
      cleanup?.();
      if (view !== null) {
        const gone = view;
        void bootBevy().then((wasm) => wasm.destroyView(gone));
        bevySentinels().__officexrBevyViews = bevySentinels().__officexrBevyViews?.filter((v) => v !== gone);
      }
    };
    // A view lives as long as its canvas, so onView runs once.
  }, []);
  return <canvas id={id} ref={canvas} style={canvasStyle} />;
}

/** Keeps `window.__officexrBevyReady` in step with every view's readiness. */
export function useReadySentinel() {
  useEffect(() => {
    const poll = window.setInterval(() => {
      const s = bevySentinels();
      const views = s.__officexrBevyViews ?? [];
      s.__officexrBevyReady = views.length > 0 && views.every((v) => s.__officexrBevy?.isReady(v).ready);
    }, 100);
    return () => {
      window.clearInterval(poll);
      const s = bevySentinels();
      delete s.__officexrBevy;
      delete s.__officexrBevyViews;
      delete s.__officexrBevyReady;
      delete s.__officexrBevyMugshotFeetY;
      delete s.__officexrBevyPicks;
    };
  }, []);
}
