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
  // The left view draws into Bevy's WebGL2 window; the right one is read back a few slow
  // SwiftShader frames after it reports ready. A page screenshot sees both as composited.
  const boxes = await page.evaluate(() =>
    ['bevy-canvas', 'bevy-canvas-2'].map((id) => document.getElementById(id)!.getBoundingClientRect().toJSON() as DOMRect),
  );
  const pixels = async () => {
    const shot = PNG.sync.read(await page.screenshot({ timeout: 120_000 }));
    const [left, right] = boxes.map((box) => {
      const out: number[] = [];
      for (let y = Math.ceil(box.y); y < Math.floor(box.y + box.height); y++) {
        for (let x = Math.ceil(box.x); x < Math.floor(box.x + box.width); x++) {
          out.push(shot.data.readUInt32BE((y * shot.width + x) * 4));
        }
      }
      return out;
    });
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
