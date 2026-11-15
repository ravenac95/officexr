/**
 * Room editor smoke test on the Bevy view (hermetic `?test=1`): the editor
 * opens, and an Add-tool click on the canvas places a block in the
 * document. The tools themselves are tested through `useSceneEditorTools`
 * in `packages/studio/src/modes/scene-editor/__tests__/editorStore.test.tsx`,
 * and picks → tool input in `editorPicks.test.ts`; the renderer's visual
 * contract lives in `crates/officexr-render`'s visual tests.
 */

import { expect, test } from '@playwright/test';
import { forceWebgl2, goToModeHermetic, waitForBevyReady } from './helpers.ts';

// SwiftShader draws the Bevy page a frame or two a second, and each real input waits on one.
test.use({ actionTimeout: 30_000 });

const EMPTY_ROOM = {
  rooms: [
    {
      name: 'default-v2',
      doc: { schemaVersion: 5, name: 'default-v2', title: 'default-v2', updatedAt: 0, commands: [], groups: {} },
    },
  ],
};

test('a canvas click with the Add tool places a block', async ({ page }) => {
  test.setTimeout(240_000);
  await forceWebgl2(page);
  await goToModeHermetic(page, 'room', EMPTY_ROOM);
  await waitForBevyReady(page);
  await expect(page.getByText('History (0)')).toBeVisible();

  await page.check('#room-show-layout-objects');
  await page.click('button[title="Blue block"]');

  // The orbit camera starts on the empty room's origin, so the canvas centre is floor.
  const canvas = page.locator('canvas').first();
  await expect(async () => {
    await canvas.click();
    await expect(page.getByText('History (1)')).toBeVisible({ timeout: 2_000 });
  }).toPass({ timeout: 60_000 });
});
