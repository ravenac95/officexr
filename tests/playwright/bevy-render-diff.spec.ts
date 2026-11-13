/**
 * Evidence, not a gate: renders a map with the studio's `?op=render` page and
 * with `crates/officexr-render`'s `render` binary from the same camera and the
 * editor lighting preset, then writes both PNGs, a side-by-side and the
 * `pixelDiffCount` plus each renderer's mean frame time.
 *
 * OFFICEXR_BEVY_DIFF=<out dir> [OFFICEXR_BEVY_DIFF_MAP=default] \
 *   pnpm exec playwright test tests/playwright/bevy-render-diff.spec.ts
 *
 * Needs the map's layouts baked (`pnpm --filter @officexr/world bake:layout`).
 */

import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { expect, test } from '@playwright/test';
import { PNG } from 'pngjs';
import { frameCamera, type Bounds } from '../../packages/studio/src/modes/headless/renderFraming.ts';
import { pixelDiffCount } from './helpers.ts';

const OUT_DIR = process.env.OFFICEXR_BEVY_DIFF;
const MAP = process.env.OFFICEXR_BEVY_DIFF_MAP ?? 'default';
const SIZE = { width: 1024, height: 768 };
const DIFF_THRESHOLD = 30;
const FRAME_SAMPLES = 120;
const ROOT = path.resolve(__dirname, '../..');

test.skip(!OUT_DIR, 'set OFFICEXR_BEVY_DIFF=<out dir> to run');

test(`three.js vs Bevy render of map "${MAP}"`, async ({ page }) => {
  test.setTimeout(20 * 60_000);
  const out = (name: string) => path.resolve(OUT_DIR!, name);
  fs.mkdirSync(OUT_DIR!, { recursive: true });

  await page.goto(`/?op=render&target=map&name=${MAP}&camera=iso&w=${SIZE.width}&h=${SIZE.height}`);
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
  execFileSync('pnpm', ['--filter', '@officexr/world', 'export:render-scene', MAP, scene, '--editor-lighting'], {
    cwd: ROOT,
    stdio: 'inherit',
  });
  const bevyStdout = execFileSync(
    'cargo',
    [
      'run', '-q', '-p', 'officexr-render', '--bin', 'render', '--',
      scene, out('bevy.png'), `${SIZE.width}x${SIZE.height}`,
      camera.position.join(','), camera.target.join(','), String(camera.fov),
    ],
    { cwd: ROOT, encoding: 'utf-8', stdio: ['ignore', 'pipe', 'inherit'] },
  );
  const bevyFrameMs = Number(/frame_ms=([\d.]+)/.exec(bevyStdout)?.[1]);

  const three = PNG.sync.read(threePng);
  const bevy = PNG.sync.read(fs.readFileSync(out('bevy.png')));
  const diff = pixelDiffCount(three.data, bevy.data, DIFF_THRESHOLD);

  const side = new PNG({ width: three.width * 2, height: three.height });
  PNG.bitblt(three, side, 0, 0, three.width, three.height, 0, 0);
  PNG.bitblt(bevy, side, 0, 0, bevy.width, bevy.height, three.width, 0);
  fs.writeFileSync(out('side-by-side.png'), PNG.sync.write(side));

  const result = { map: MAP, size: SIZE, camera, diffThreshold: DIFF_THRESHOLD, pixelDiffCount: diff, totalPixels: SIZE.width * SIZE.height, threeFrameMs, bevyFrameMs };
  fs.writeFileSync(out('result.json'), `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result));
});
