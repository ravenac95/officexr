/** `?op=bevy`: the studio boots `crates/officexr-render-web` on the current map. */

import { test, expect } from '@playwright/test';

test('op=bevy boots the Bevy renderer', async ({ page }) => {
  test.setTimeout(180_000);
  await page.goto('/?op=bevy');
  await expect(page.locator('#bevy-canvas')).toBeVisible();
  await page.waitForFunction(() => (window as { __officexrBevyReady?: boolean }).__officexrBevyReady === true, null, {
    timeout: 170_000,
  });
});
