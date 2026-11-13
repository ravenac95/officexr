/** `?op=bevy`: the studio boots `crates/officexr-render-web` on the current map. */

import { test, expect } from '@playwright/test';

test('op=bevy boots the Bevy renderer', async ({ page }) => {
  test.setTimeout(180_000);
  // A wasm panic surfaces as a pageerror; fail on it instead of waiting out the boot timeout.
  const pageError = new Promise<never>((_, reject) => page.once('pageerror', reject));
  await page.goto('/?op=bevy');
  await expect(page.locator('#bevy-canvas')).toBeVisible();
  const ready = page.waitForFunction(
    () => (window as { __officexrBevyReady?: boolean }).__officexrBevyReady === true,
    null,
    { timeout: 170_000 },
  );
  await Promise.race([ready, pageError]);
});
