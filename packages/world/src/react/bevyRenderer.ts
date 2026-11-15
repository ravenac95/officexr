import { loadRenderer, type RendererModule } from '@officexr/render-wasm';
import { modelUrl } from '../app/asset-url.ts';

let booted: Promise<RendererModule> | null = null;
let playCanvasElement: HTMLCanvasElement | null = null;

/**
 * Bevy owns its event loop for the page's lifetime, so it boots once, with the first caller's
 * `primary` canvas, if any, as its primary window.
 */
export function bootBevy(primary?: HTMLCanvasElement): Promise<RendererModule> {
  booted ??= loadRenderer()
    .then((wasm) => {
      const origin = modelUrl('/models/').replace(/\/models\/$/, '') || window.location.origin;
      wasm.init(origin, primary);
      return wasm;
    })
    .catch((error: unknown) => {
      // A failed load (a dropped connection) is retried by the next mount.
      booted = null;
      throw error;
    });
  return booted;
}

/**
 * Bevy's primary canvas, one per page, shared by the play view and the studio's scene editor (one
 * mounted at a time): Bevy draws its primary window straight into the canvas it booted with, so a
 * remounted view moves this element instead of making a new one.
 */
export function playCanvas(): HTMLCanvasElement {
  playCanvasElement ??= document.createElement('canvas');
  return playCanvasElement;
}

/** The URL Bevy loads a baked layout from: absolute, as Bevy resolves relative paths against the model origin. */
export function bakedLayoutGlbUrl(path: string): string {
  return new URL(path.endsWith('.glb') ? path : `${path}.glb`, window.location.href).href;
}

/** The renderer's `blob://` asset path for one of this page's `blob:` URLs; `ext` picks Bevy's loader. */
export function blobAssetPath(blobUrl: string, ext: string): string {
  return `blob://${blobUrl.slice(blobUrl.lastIndexOf('/') + 1)}.${ext}`;
}
