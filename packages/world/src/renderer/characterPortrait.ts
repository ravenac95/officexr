import type { RendererModule, ViewId, WorldDiff } from '@officexr/render-wasm';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import { DEFAULT_EDITOR_LIGHTING } from '../app/editor-lighting.ts';
import { cachedCharacterPortrait, storeCharacterPortrait } from '../characters/portraitCache.ts';
import { encodePortrait } from '../characters/portraitEncoding.ts';
import { BODY_Y } from '../physics/rules.ts';
import { bootBevy } from '../react/bevyRenderer.ts';

export const PORTRAIT_PX = 160;
/** The share of the model's height the portrait shows, from the top of the head. */
const HEAD_AND_SHOULDERS = 0.58;
const FOV_DEG = 28;
const CHAR_RADIUS = 0.4;
const SEE_THROUGH = '#00000000';
/** Frames drawn after the model is ready before the shot, so its materials have rendered. */
const SETTLE_FRAMES = 3;
const LOAD_TIMEOUT_MS = 30_000;

/** A key light from the front-right above, as `characterPortrait.ts`'s studio rig. */
const PORTRAIT_LIGHTING = { ...DEFAULT_EDITOR_LIGHTING, sunPosition: [1.5, 3, 4] as [number, number, number], sunIntensity: 2.2, ambientFillIntensity: 0.6 };

const pending = new Map<string, Promise<string>>();
let queue: Promise<unknown> = Promise.resolve();

/** The camera that frames the head and shoulders of a model `height` tall, standing at the origin facing +Z. */
export function portraitCamera(height: number) {
  const window = height * HEAD_AND_SHOULDERS;
  const centreY = height - window / 2;
  const distance = window / 2 / Math.tan((FOV_DEG * Math.PI) / 360);
  return {
    position: [0, centreY + window * 0.12, distance] as [number, number, number],
    lookAt: [0, centreY, 0] as [number, number, number],
    fovDeg: FOV_DEG,
    near: 0.05,
    far: 50,
  };
}

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

/** An off-screen preview view on its own canvas, for one shot. */
function portraitView(wasm: RendererModule): { view: ViewId; dispose(): void } {
  const host = document.createElement('div');
  // On screen but invisible: a hidden canvas would pause the view.
  Object.assign(host.style, {
    position: 'fixed',
    left: '0',
    top: '0',
    width: `${PORTRAIT_PX}px`,
    height: `${PORTRAIT_PX}px`,
    opacity: '0',
    pointerEvents: 'none',
  });
  const canvas = document.createElement('canvas');
  Object.assign(canvas.style, { width: '100%', height: '100%', display: 'block' });
  host.append(canvas);
  document.body.append(host);
  const view = wasm.createView(canvas, 'preview');
  return {
    view,
    dispose() {
      wasm.destroyView(view);
      host.remove();
    },
  };
}

/** Renders `url`'s character in Bevy and returns the frame as an image of the canvas's size. */
async function capture(url: string): Promise<HTMLCanvasElement> {
  const wasm = await bootBevy();
  const { view, dispose } = portraitView(wasm);
  try {
    const diff: WorldDiff = {
      replace: true,
      lighting: PORTRAIT_LIGHTING,
      background: DEFAULT_WORLD_CONSTANTS.background,
      clearColor: SEE_THROUGH,
      kinds: [],
      objects: { instances: [] },
    };
    wasm.setWorld(view, JSON.stringify(diff));
    wasm.setPlayers(view, {
      // Bind pose, facing the camera: yaw π turns the model's +Z face back toward +Z.
      players: [{ id: 'portrait', character: url, pos: [0, -(BODY_Y - CHAR_RADIUS), 0], yaw: Math.PI, vel: [0, 0, 0], name: '', airborne: false }],
      charRadius: CHAR_RADIUS,
      playerSpeed: 3,
      runSpeed: 6,
      paused: true,
    });
    const started = performance.now();
    while (!wasm.isReady(view).ready) {
      if (performance.now() - started > LOAD_TIMEOUT_MS) {
        throw new Error(`the portrait of ${url} did not load`);
      }
      await nextFrame();
    }
    // The model's height is measured once it binds, and reported the frame after.
    const height = wasm.playerAnchors(view).find((a) => a.id === 'portrait')?.height;
    if (height === undefined) {
      throw new Error(`no anchor for the portrait of ${url}`);
    }
    wasm.setCamera(view, portraitCamera(height));
    for (let i = 0; i < SETTLE_FRAMES; i++) {
      await nextFrame();
    }
    const blob = await wasm.captureView(view);
    const bitmap = await createImageBitmap(blob);
    const out = document.createElement('canvas');
    out.width = PORTRAIT_PX;
    out.height = PORTRAIT_PX;
    out.getContext('2d')?.drawImage(bitmap, 0, 0, PORTRAIT_PX, PORTRAIT_PX);
    bitmap.close();
    return out;
  } finally {
    dispose();
  }
}

/** One shot at a time: each makes and destroys its own view. */
function enqueue<T>(job: () => Promise<T>): Promise<T> {
  const run = queue.then(job);
  queue = run.catch(() => undefined);
  return run;
}

/** The portrait for `url`: the cached one, else one rendered with Bevy and cached. */
export function renderCharacterPortrait(url: string): Promise<string> {
  const known = cachedCharacterPortrait(url);
  if (known) {
    return Promise.resolve(known);
  }
  let job = pending.get(url);
  if (!job) {
    job = enqueue(() => capture(url)).then(
      (canvas) => {
        const image = encodePortrait(canvas);
        storeCharacterPortrait(url, image);
        pending.delete(url);
        return image;
      },
      (error: unknown) => {
        pending.delete(url);
        throw error;
      },
    );
    pending.set(url, job);
  }
  return job;
}

/** Renders `url` as a data URL of type `mime`, bypassing every cache. For the build script that bakes the built-in portraits. */
export function bakeCharacterPortrait(url: string, mime: string): Promise<string> {
  return enqueue(() => capture(url)).then((canvas) => canvas.toDataURL(mime));
}
