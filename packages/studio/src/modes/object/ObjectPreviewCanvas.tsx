import React, { useEffect, useRef } from 'react';
import type { WorldObjectKind } from '@officexr/world';
import { BevyPreview, type PreviewView } from '@officexr/world/react';
import { publishFrameStats } from '../../perf/frame-stats.ts';

/** What `pnpm gen:thumbnails` drives the `?thumbnailMode=true` preview with. */
export interface ThumbnailSentinel {
  /** The kind the preview has been handed. */
  kindId: string | null;
  /** Every model of that kind has loaded and drawn. */
  ready(): boolean;
  capture(): Promise<Blob>;
}

const THUMBNAIL_PX = 128;

interface ObjectPreviewCanvasProps {
  kind: WorldObjectKind | null;
}

/**
 * The Object editor's view: one instance of the selected kind, slowly spinning on the reference grid
 * (paused while the pointer is over it), with live material overrides. Left-drag orbits, the wheel zooms.
 *
 * With `?thumbnailMode=true` it is a still 128×128 view on a see-through background with no grid, and
 * `window.__officexrThumbnail` (a {@link ThumbnailSentinel}) captures it.
 */
export function ObjectPreviewCanvas({ kind }: ObjectPreviewCanvasProps) {
  const thumbnailMode = new URLSearchParams(window.location.search).get('thumbnailMode') === 'true';
  const preview = useRef<PreviewView | null>(null);

  useEffect(() => {
    if (!thumbnailMode) {
      return;
    }
    const sentinel: ThumbnailSentinel = {
      kindId: kind?.id ?? null,
      ready: () => !!preview.current && preview.current.wasm.isReady(preview.current.view).ready,
      capture: () => {
        if (!preview.current) {
          return Promise.reject(new Error('the preview has no view yet'));
        }
        return preview.current.wasm.captureView(preview.current.view);
      },
    };
    const w = window as { __officexrThumbnail?: ThumbnailSentinel };
    w.__officexrThumbnail = sentinel;
    return () => {
      if (w.__officexrThumbnail === sentinel) {
        delete w.__officexrThumbnail;
      }
    };
  }, [thumbnailMode, kind]);

  return (
    <BevyPreview
      subject={{ type: 'kind', kind, spin: !thumbnailMode }}
      transparent={thumbnailMode}
      onView={(view) => {
        preview.current = view;
      }}
      // The capture script's view mustn't publish to (or clear) the live footer readout.
      onFrameStats={thumbnailMode ? undefined : publishFrameStats}
      style={
        thumbnailMode
          ? { width: THUMBNAIL_PX, height: THUMBNAIL_PX }
          : { width: '100%', height: '100%' }
      }
    />
  );
}
