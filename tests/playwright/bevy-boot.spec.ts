/** `?op=bevy`: the studio boots `crates/officexr-render-web` on the current map. */

import { test, expect, type Page } from '@playwright/test';
import { PNG } from 'pngjs';
import { forceWebgl2 } from './helpers.ts';

test('op=bevy boots the Bevy renderer', async ({ page }) => {
  test.setTimeout(180_000);
  // A wasm panic surfaces as a pageerror; fail on it instead of waiting out the boot timeout.
  const pageError = new Promise<never>((_, reject) => page.once('pageerror', reject));
  await forceWebgl2(page);
  await page.goto('/?op=bevy');
  await expect(page.locator('#bevy-canvas')).toBeVisible();
  const ready = page.waitForFunction(
    () => (window as { __officexrBevyReady?: boolean }).__officexrBevyReady === true,
    null,
    { timeout: 170_000 },
  );
  await Promise.race([ready, pageError]);
  expect(await page.evaluate(() => (window as { __officexrRenderBackend?: string }).__officexrRenderBackend)).toBe('webgl2');
});

test('op=bevy&views=2 draws two live canvases from one Bevy app', async ({ page }) => {
  test.setTimeout(540_000);
  const pageError = new Promise<never>((_, reject) => page.once('pageerror', reject));
  await forceWebgl2(page);
  await page.goto('/?op=bevy&views=2');
  const ready = page.waitForFunction(
    () => (window as { __officexrBevyReady?: boolean }).__officexrBevyReady === true,
    null,
    { timeout: 230_000 },
  );
  await Promise.race([ready, pageError]);
  expect(await page.evaluate(() => (window as { __officexrBevyViews?: number[] }).__officexrBevyViews?.length)).toBe(2);
  // The right view is read back, a few slow SwiftShader frames after it reports ready.
  const pixels = async () => {
    const [left, right] = await canvasPixels(page, ['#bevy-canvas', '#bevy-canvas-2']);
    const differing = left.filter((p, i) => p !== right[i]).length;
    return {
      sameSize: left.length === right.length,
      colours: [new Set(left).size, new Set(right).size],
      differing: differing / left.length,
    };
  };
  await expect
    .poll(
      async () => {
        const { sameSize, colours, differing } = await pixels();
        return sameSize && colours[0] > 16 && colours[1] > 16 && differing > 0.2;
      },
      { timeout: 300_000, intervals: [2_000] },
    )
    .toBe(true);
});

/**
 * Each canvas's pixels from one page screenshot, which sees WebGL and read-back canvases alike as
 * composited. Element screenshots wait on a compositor frame, too slow under SwiftShader. `fromTop`
 * skips that fraction of each canvas's height.
 */
async function canvasPixels(page: Page, selectors: string[], fromTop = 0) {
  const boxes = await page.evaluate(
    (all) => all.map((s) => document.querySelector(s)!.getBoundingClientRect().toJSON() as DOMRect),
    selectors,
  );
  const shot = PNG.sync.read(await page.screenshot({ timeout: 120_000 }));
  return boxes.map((box) => {
    const out: number[] = [];
    for (let y = Math.ceil(box.y + box.height * fromTop); y < Math.floor(box.y + box.height); y++) {
      for (let x = Math.ceil(box.x); x < Math.floor(box.x + box.width); x++) {
        out.push(shot.data.readUInt32BE((y * shot.width + x) * 4));
      }
    }
    return out;
  });
}

test('op=bevy draws the sky, not a blank canvas, when the map fails to load', async ({ page }) => {
  test.setTimeout(240_000);
  await forceWebgl2(page);
  // As on a Pages preview with no studio API behind it.
  await page.route('**/api/maps**', (route) => route.fulfill({ status: 404, body: 'not found' }));
  await page.goto('/?op=bevy&test=1');
  await expect(page.getByRole('alert')).toContainText('Could not load map', { timeout: 60_000 });
  // The sky, not a flat clear colour; the lower half is clear of the error text.
  const drawn = async () => {
    const stats = await page.evaluate(() => {
      const w = window as unknown as {
        __officexrBevy?: { frameStats: (v: number) => { drawCalls: number } };
        __officexrBevyViews?: number[];
      };
      const view = w.__officexrBevyViews?.[0];
      return view === undefined ? 0 : w.__officexrBevy!.frameStats(view).drawCalls;
    });
    const colours = new Set((await canvasPixels(page, ['#bevy-canvas'], 0.5))[0]).size;
    return stats > 0 && colours > 1;
  };
  await expect.poll(drawn, { timeout: 180_000, intervals: [2_000] }).toBe(true);
});

/**
 * Where `?op=bevy&views=2` settles its preview character's feet, once its view has drawn the same
 * frame for a few seconds: the character is settled once and drawn paused, never falling.
 */
async function previewFeetY(page: Page, query: string) {
  await forceWebgl2(page);
  await page.goto(`/?op=bevy&views=2${query}`);
  const feetY = await page.waitForFunction(
    () => (window as { __officexrBevyMugshotFeetY?: number }).__officexrBevyMugshotFeetY,
    null,
    { timeout: 230_000 },
  );
  const preview = async () => (await canvasPixels(page, ['#bevy-canvas-2']))[0];
  const still = async () => {
    const before = await preview();
    await page.waitForTimeout(3_000);
    const after = await preview();
    return new Set(before).size > 16 && before.every((p, i) => p === after[i]);
  };
  await expect.poll(still, { timeout: 180_000, intervals: [1_000] }).toBe(true);
  return feetY.jsonValue();
}

// The mugshot's 2 m cubes sit at voxel y -2: their tops are at y = 1.
const CUBE_TOP = 1;

test('op=bevy&views=2 stands the preview character on the cubes', async ({ page }) => {
  test.setTimeout(300_000);
  expect(await previewFeetY(page, '')).toBeCloseTo(CUBE_TOP, 1);
});

// With the bundled fallback catalog, as on a Pages preview.
test('op=bevy&views=2&test=1 stands the preview character on the cubes', async ({ page }) => {
  test.setTimeout(300_000);
  expect(await previewFeetY(page, '&test=1')).toBeCloseTo(CUBE_TOP, 1);
});
