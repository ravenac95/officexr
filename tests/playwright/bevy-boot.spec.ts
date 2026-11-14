/** `?op=bevy`: the studio boots `crates/officexr-render-web` on the current map. */

import { test, expect } from '@playwright/test';
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

/** Distinct colours among a PNG's pixels, and the share of pixels that differ from another PNG of the same size. */
function compare(a: PNG, b: PNG) {
  const colours = (png: PNG) => {
    const seen = new Set<number>();
    for (let i = 0; i < png.data.length; i += 4) {
      seen.add(png.data.readUInt32BE(i));
    }
    return seen.size;
  };
  let differing = 0;
  for (let i = 0; i < a.data.length; i += 4) {
    if (a.data.readUInt32BE(i) !== b.data.readUInt32BE(i)) {
      differing++;
    }
  }
  return { colours: [colours(a), colours(b)], differing: differing / (a.data.length / 4) };
}

test('op=bevy&views=2 draws two live canvases from one Bevy app', async ({ page }) => {
  test.setTimeout(240_000);
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
  const [left, right] = await Promise.all(
    ['#bevy-canvas', '#bevy-canvas-2'].map(async (selector) => PNG.sync.read(await page.locator(selector).screenshot())),
  );
  expect([left.width, left.height]).toEqual([right.width, right.height]);
  const { colours, differing } = compare(left, right);
  expect(colours[0]).toBeGreaterThan(16);
  expect(colours[1]).toBeGreaterThan(16);
  expect(differing).toBeGreaterThan(0.2);
});
