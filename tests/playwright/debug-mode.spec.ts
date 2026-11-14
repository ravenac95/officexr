/**
 * Debug mode smoke tests: the play view renders through Bevy, the local
 * player walks on the keyboard, the free-fly toggle frees the camera from
 * the player, and switching maps loads the new map's baked layout. Pixels
 * are only checked for "not blank"; the renderer's visual contract lives in
 * `crates/officexr-render`'s visual tests.
 */

import { expect, test, type Page } from '@playwright/test';
import { distinctColorsInCanvas, forceWebgl2, waitForBevyReady } from './helpers.ts';
import { goToDebugWithMap } from './motion-helpers.ts';

const SELF_ID = 'local-player';

type Pos = { x: number; y: number; z: number };

const storeSummary = (page: Page) =>
  page.evaluate((selfId: string) => {
    type State = { players: Record<string, { pos: Pos }>; worldObjects: { instances: { id: string }[] } };
    const s = (window as unknown as { __OFFICE_STORE__?: { getState: () => State } }).__OFFICE_STORE__?.getState();
    return {
      self: s?.players[selfId]?.pos ?? null,
      playerCount: Object.keys(s?.players ?? {}).length,
      layoutCount: (s?.worldObjects.instances ?? []).filter((i) => i.id.includes('/layout/')).length,
    };
  }, SELF_ID);

/** Holds W on the world and returns how far the local player moved across the ground. */
async function walkForward(page: Page): Promise<number> {
  const before = (await storeSummary(page)).self!;
  // A synthetic mousedown hands the keyboard to the world: a real click waits on frames SwiftShader is slow to draw.
  await page.evaluate(() => document.querySelector('canvas')!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 })));
  await page.keyboard.down('KeyW');
  await page.waitForTimeout(3_000);
  await page.keyboard.up('KeyW');
  const after = (await storeSummary(page)).self!;
  return Math.hypot(after.x - before.x, after.z - before.z);
}

test('Debug mode renders through Bevy; the player walks, and stays put in free-fly', async ({ page }) => {
  test.setTimeout(240_000);
  await forceWebgl2(page);
  await goToDebugWithMap(page, 'default');
  await waitForBevyReady(page);

  const summary = await storeSummary(page);
  expect(summary.self, 'local player').not.toBeNull();
  expect(summary.playerCount, 'self + at least one bot').toBeGreaterThan(1);
  await expect.poll(() => distinctColorsInCanvas(page.locator('canvas').first()), { timeout: 30_000 }).toBeGreaterThan(8);

  expect(await walkForward(page), 'walked on W').toBeGreaterThan(0.5);

  await page.getByRole('switch', { name: 'free-fly' }).dispatchEvent('click');
  await expect(page.getByRole('switch', { name: 'free-fly' })).toBeChecked();
  expect(await walkForward(page), 'W flies the camera, not the player').toBeLessThan(0.05);
});

test('Map switch in Debug loads the new map and its baked layout', async ({ page, request }) => {
  test.setTimeout(300_000);
  const a = 'default';
  const b = 'long_corridor';
  const maps = (await request.get('/api/maps').then((r) => r.json())) as { maps?: { name: string }[] };
  const names = new Set((maps.maps ?? []).map((m) => m.name));
  test.skip(!names.has(a) || !names.has(b), `needs maps "${a}" and "${b}" on disk`);

  const bakedFetches = new Set<string>();
  page.on('response', (r) => {
    const match = /\/api\/baked-layouts\/([^?]+?)(\.glb)?(\?|$)/.exec(r.url());
    if (match) {
      bakedFetches.add(decodeURIComponent(match[1]!));
    }
  });
  await forceWebgl2(page);
  await goToDebugWithMap(page, a);
  await waitForBevyReady(page);
  expect((await storeSummary(page)).layoutCount, `map "${a}" layout instances`).toBeGreaterThan(0);
  const before = new Set(bakedFetches);

  // Synthetic pointer events: Radix listens for pointerdown.
  await page.evaluate(() => {
    const trigger = document.querySelector('[role="combobox"]') as HTMLElement;
    trigger.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }));
    trigger.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, button: 0, pointerType: 'mouse' }));
    trigger.click();
  });
  await page.evaluate((nextMap: string) => {
    const target = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => o.textContent?.trim() === nextMap);
    if (!target) {
      throw new Error(`no option ${nextMap}`);
    }
    target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }));
    target.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, button: 0, pointerType: 'mouse' }));
    target.click();
  }, b);

  await expect
    .poll(() => [...bakedFetches].filter((n) => !before.has(n)), { message: `a new baked layout for "${b}"`, timeout: 60_000 })
    .not.toEqual([]);
  expect((await storeSummary(page)).layoutCount, `map "${b}" layout instances`).toBeGreaterThan(0);
});
