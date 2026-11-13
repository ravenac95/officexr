/**
 * Evidence, not a gate: renders the `long_corridor` layout, placed off the
 * origin with a quarter turn, with the studio's `?op=render` page and with
 * `crates/officexr-render`'s `render` binary from the same camera and the
 * editor lighting preset. Writes both PNGs, a side-by-side, `pixelDiffCount`
 * over the whole frame and over the scene's on-screen box (which leaves out
 * most of the three.js grid), and each renderer's mean frame time.
 *
 * OFFICEXR_BEVY_DIFF=<out dir> pnpm exec playwright test tests/playwright/bevy-render-diff.spec.ts
 *
 * Needs the layout baked: `pnpm --filter @officexr/world bake:layout long_corridor`.
 */

import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { expect, test } from '@playwright/test';
import { PNG } from 'pngjs';
import { frameCamera, type Bounds } from '../../packages/studio/src/modes/headless/renderFraming.ts';
import { pixelDiffCount } from './helpers.ts';

const OUT_DIR = process.env.OFFICEXR_BEVY_DIFF;
const SIZE = { width: 1024, height: 768 };
const DIFF_THRESHOLD = 30;
const FRAME_SAMPLES = 120;
const ROOT = path.resolve(__dirname, '../..');
const WORLD = path.resolve(ROOT, 'packages/world');
const ROOM = 'default-v2';
const LAYOUT = 'long_corridor';
const PLACEMENT = { position: [6, 0, -4], rotationY: 1 };
const MAP = 'bevy-diff';
const BACKGROUND = [0x1b, 0x1d, 0x22];

const readJson = (file: string) => JSON.parse(fs.readFileSync(path.resolve(WORLD, file), 'utf-8')) as object;

const SEED = {
  maps: [
    {
      name: MAP,
      doc: {
        ...readJson(`maps/${LAYOUT}.json`),
        name: MAP,
        rooms: [{ id: 'room-1', roomName: ROOM, ...PLACEMENT }],
      },
    },
  ],
  rooms: [{ name: ROOM, doc: readJson(`rooms/${ROOM}.json`) }],
  layouts: [{ name: LAYOUT, doc: readJson(`layouts/${LAYOUT}.json`) }],
};

/** Smallest box holding every pixel that isn't the background. */
function sceneBox(png: PNG) {
  const box = { minX: png.width, minY: png.height, maxX: -1, maxY: -1 };
  for (let i = 0; i < png.data.length; i += 4) {
    const dist = BACKGROUND.reduce((sum, c, k) => sum + Math.abs(png.data[i + k] - c), 0);
    if (dist > DIFF_THRESHOLD) {
      const x = (i / 4) % png.width;
      const y = Math.floor(i / 4 / png.width);
      box.minX = Math.min(box.minX, x);
      box.maxX = Math.max(box.maxX, x);
      box.minY = Math.min(box.minY, y);
      box.maxY = Math.max(box.maxY, y);
    }
  }
  return box;
}

function crop(png: PNG, box: ReturnType<typeof sceneBox>): PNG {
  const out = new PNG({ width: box.maxX - box.minX + 1, height: box.maxY - box.minY + 1 });
  PNG.bitblt(png, out, box.minX, box.minY, out.width, out.height, 0, 0);
  return out;
}

test.skip(!OUT_DIR, 'set OFFICEXR_BEVY_DIFF=<out dir> to run');

test(`three.js vs Bevy render of a placed "${LAYOUT}" layout`, async ({ page }) => {
  test.setTimeout(20 * 60_000);
  const out = (name: string) => path.resolve(OUT_DIR!, name);
  fs.mkdirSync(OUT_DIR!, { recursive: true });

  await page.addInitScript((seed) => {
    (window as unknown as { __OFFICEXR_TEST_SEED__: unknown }).__OFFICEXR_TEST_SEED__ = seed;
  }, SEED);
  await page.route(new RegExp(`/api/baked-layouts/${LAYOUT}(\\?.*)?$`), (route) =>
    route.fulfill({ path: path.resolve(WORLD, `baked-layouts/${LAYOUT}.glb`), contentType: 'model/gltf-binary' }),
  );
  await page.goto(`/?test=1&op=render&target=map&name=${MAP}&camera=iso&w=${SIZE.width}&h=${SIZE.height}`);
  await page.waitForFunction(() => '__officexrRenderReady' in window || '__officexrRenderError' in window, null, {
    timeout: 120_000,
  });
  const ready = await page.evaluate(() => (window as unknown as { __officexrRenderReady?: { bounds: Bounds } }).__officexrRenderReady);
  expect(ready, 'render page reported an error').toBeDefined();
  const threePng = await page.locator('canvas').screenshot();
  fs.writeFileSync(out('three.png'), threePng);
  const threeFrameMs = await page.evaluate(
    (n) =>
      new Promise<number>((resolve) => {
        let first = 0;
        let frames = 0;
        const tick = (t: number) => {
          if (frames === 0) {
            first = t;
          }
          if (frames++ === n) {
            resolve((t - first) / n);
          } else {
            requestAnimationFrame(tick);
          }
        };
        requestAnimationFrame(tick);
      }),
    FRAME_SAMPLES,
  );

  const camera = frameCamera({ type: 'iso' }, ready!.bounds, SIZE);
  if (camera.type !== 'perspective') {
    throw new Error('iso framing is perspective');
  }
  const scene = out('scene.json');
  execFileSync('pnpm', ['--filter', '@officexr/world', 'export:render-scene', LAYOUT, scene, '--editor-lighting'], {
    cwd: ROOT,
    stdio: 'inherit',
  });
  // The exporter reads maps from disk, so the seeded map's placement is applied here.
  const sceneJson = JSON.parse(fs.readFileSync(scene, 'utf-8')) as { bakedLayouts: object[] };
  sceneJson.bakedLayouts = sceneJson.bakedLayouts.map((p) => ({ ...p, ...PLACEMENT }));
  fs.writeFileSync(scene, JSON.stringify(sceneJson));
  const bevyStdout = execFileSync(
    'cargo',
    [
      'run', '-q', '-p', 'officexr-render', '--bin', 'render', '--',
      scene, out('bevy.png'), `${SIZE.width}x${SIZE.height}`,
      camera.position.join(','), camera.target.join(','),
      String(camera.fov), String(camera.near), String(camera.far),
    ],
    { cwd: ROOT, encoding: 'utf-8', stdio: ['ignore', 'pipe', 'inherit'] },
  );
  const bevyFrameMs = Number(/frame_ms=([\d.]+)/.exec(bevyStdout)?.[1]);

  const three = PNG.sync.read(threePng);
  const bevy = PNG.sync.read(fs.readFileSync(out('bevy.png')));
  const box = sceneBox(bevy);
  const boxDiff = pixelDiffCount(crop(three, box).data, crop(bevy, box).data, DIFF_THRESHOLD);

  const side = new PNG({ width: three.width * 2, height: three.height });
  PNG.bitblt(three, side, 0, 0, three.width, three.height, 0, 0);
  PNG.bitblt(bevy, side, 0, 0, bevy.width, bevy.height, three.width, 0);
  fs.writeFileSync(out('side-by-side.png'), PNG.sync.write(side));

  const result = {
    layout: LAYOUT,
    placement: PLACEMENT,
    size: SIZE,
    camera,
    diffThreshold: DIFF_THRESHOLD,
    pixelDiffCount: pixelDiffCount(three.data, bevy.data, DIFF_THRESHOLD),
    totalPixels: SIZE.width * SIZE.height,
    sceneBox: box,
    sceneBoxDiffCount: boxDiff,
    sceneBoxPixels: (box.maxX - box.minX + 1) * (box.maxY - box.minY + 1),
    threeFrameMs,
    bevyFrameMs,
  };
  fs.writeFileSync(out('result.json'), `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result));
});
