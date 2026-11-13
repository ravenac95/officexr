/** `?sim=rust`: the studio's local player walks on the wasm build of `crates/officexr-sim`. */

import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import { waitForCanvasReady } from './helpers.ts';
import { goToDebugWithMap, setFixedCamera } from './motion-helpers.ts';

const SELF_ID = 'local-player';

type Pos = { x: number; y: number; z: number };

const selfPos = (page: Page): Promise<Pos | null> =>
  page.evaluate((selfId: string) => {
    const w = window as unknown as {
      __OFFICE_STORE__?: { getState: () => { players: Record<string, { pos: Pos } | undefined> } };
    };
    return w.__OFFICE_STORE__?.getState().players[selfId]?.pos ?? null;
  }, SELF_ID);

test('sim=rust: the local player walks under keyboard input', async ({ page }) => {
  test.setTimeout(90_000);
  const wasmLoaded = page.waitForResponse((r) => /officexr_sim_wasm_bg.*\.wasm/.test(r.url()) && r.ok());
  await goToDebugWithMap(page, 'default', 'rust');
  await wasmLoaded;
  await waitForCanvasReady(page, 0, 30_000);
  await setFixedCamera(page, { azimuthDeg: 90, movementYawOffsetDeg: 0 });

  const before = await selfPos(page);
  expect(before).not.toBeNull();

  await page.locator('canvas').first().click({ position: { x: 200, y: 200 } });
  await page.keyboard.down('KeyW');
  await page.waitForTimeout(1_500);
  await page.keyboard.up('KeyW');

  const after = await selfPos(page);
  const moved = Math.hypot(after!.x - before!.x, after!.z - before!.z);
  expect(moved, `moved from ${JSON.stringify(before)} to ${JSON.stringify(after)}`).toBeGreaterThan(1);
});
