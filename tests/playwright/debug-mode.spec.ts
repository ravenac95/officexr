/**
 * Debug mode: the local player walks on the keyboard, the free-fly toggle
 * frees the camera from the player, and switching maps loads the new map's
 * baked layout. That it draws is `rendering-smoke.spec.ts`'s.
 */

import { expect, test, type Page } from '@playwright/test';
import { forceWebgl2, goToDebugWithMap, waitForBevyReady } from './helpers.ts';

const SELF_ID = 'local-player';

// SwiftShader draws the Bevy page a frame or two a second, and each real input waits on one.
test.use({ actionTimeout: 30_000 });

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
  await page.locator('canvas').first().click({ position: { x: 200, y: 200 } });
  await page.keyboard.down('KeyW');
  await page.waitForTimeout(3_000);
  await page.keyboard.up('KeyW');
  const after = (await storeSummary(page)).self!;
  return Math.hypot(after.x - before.x, after.z - before.z);
}

test('Debug mode: the player walks, and stays put in free-fly', async ({ page }) => {
  test.setTimeout(240_000);
  await forceWebgl2(page);
  await goToDebugWithMap(page, 'default');
  await waitForBevyReady(page);

  const summary = await storeSummary(page);
  expect(summary.self, 'local player').not.toBeNull();
  expect(summary.playerCount, 'self + at least one bot').toBeGreaterThan(1);

  expect(await walkForward(page), 'walked on W').toBeGreaterThan(0.5);

  // Real clicks: a panel control takes the keyboard, and a click on the canvas must give it back.
  const freeFly = page.getByRole('switch', { name: 'free-fly' });
  await freeFly.click();
  await expect(freeFly).toBeChecked();
  expect(await walkForward(page), 'W flies the camera, not the player').toBeLessThan(0.05);

  await freeFly.click();
  await expect(freeFly).not.toBeChecked();
  expect(await walkForward(page), 'walked again after a panel click and a canvas click').toBeGreaterThan(0.5);
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
