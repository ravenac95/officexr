import { loadRenderer, type RendererModule } from '@officexr/render-wasm';
import { modelUrl } from '../app/asset-url.ts';

let booted: Promise<RendererModule> | null = null;
let playCanvasElement: HTMLCanvasElement | null = null;

/**
 * Bevy owns its event loop for the page's lifetime, so it boots once, with the first caller's
 * `primary` canvas, if any, as its primary window.
 */
export function bootBevy(primary?: HTMLCanvasElement): Promise<RendererModule> {
  booted ??= loadRenderer().then((wasm) => {
    const origin = modelUrl('/models/').replace(/\/models\/$/, '') || window.location.origin;
    wasm.init(origin, primary);
    return wasm;
  });
  return booted;
}

/**
 * The play view's canvas, one per page: Bevy draws its primary window straight into the canvas
 * it booted with, so a remounted play view moves this element instead of making a new one.
 */
export function playCanvas(): HTMLCanvasElement {
  playCanvasElement ??= document.createElement('canvas');
  return playCanvasElement;
}
