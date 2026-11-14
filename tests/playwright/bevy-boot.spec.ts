/** `?op=bevy`: the studio boots `crates/officexr-render-web` on the current map. */

import { test, expect } from '@playwright/test';
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
  // WebGL2 views are painted into 2D canvases a few slow SwiftShader frames after they report
  // ready; reading them back beats element screenshots, which wait on a compositor frame.
  const pixels = () =>
    page.evaluate(() => {
      const [left, right] = ['bevy-canvas', 'bevy-canvas-2'].map((id) => {
        const c = document.getElementById(id) as HTMLCanvasElement;
        return c.getContext('2d')!.getImageData(0, 0, c.width, c.height).data;
      });
      const colours = (d: Uint8ClampedArray) => {
        const seen = new Set<number>();
        for (let i = 0; i < d.length; i += 4) {
          seen.add((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
        }
        return seen.size;
      };
      let differing = 0;
      for (let i = 0; i < left.length; i += 4) {
        if (left[i] !== right[i] || left[i + 1] !== right[i + 1] || left[i + 2] !== right[i + 2]) {
          differing++;
        }
      }
      return { sameSize: left.length === right.length, colours: [colours(left), colours(right)], differing: (differing * 4) / left.length };
    });
  await expect
    .poll(
      async () => {
        const { sameSize, colours, differing } = await pixels();
        return sameSize && colours[0] > 16 && colours[1] > 16 && differing > 0.2;
      },
      { timeout: 240_000, intervals: [2_000] },
    )
    .toBe(true);
});
