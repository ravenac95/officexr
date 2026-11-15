/**
 * Every studio surface boots Bevy and draws a non-blank canvas, hermetically (`?test=1`). What
 * they draw is the native harness's (`crates/officexr-render/tests/visual/`). The web app's room
 * view is the same `PlayView` as Debug mode's.
 */

import { expect, test, type Page } from '@playwright/test';
import { canvasPixels, forceWebgl2, goToModeHermetic, waitForBevyReady, type Mode } from './helpers.ts';

const block = { id: 'b1', op: 'placeObject', kindId: '__primitive_blue', position: [0, 0, 0] };

// Each editor's default document.
const ROOM = 'default-v2';
const SEED = {
  rooms: [{ name: ROOM, doc: { schemaVersion: 5, name: ROOM, title: ROOM, updatedAt: 0, commands: [block], groups: {} } }],
  layouts: [{ name: 'default', doc: { schemaVersion: 1, name: 'default', updatedAt: 0, commands: [block], groups: {} } }],
  maps: [
    {
      name: 'default',
      doc: {
        schemaVersion: 2,
        name: 'default',
        title: 'default',
        updatedAt: 0,
        rooms: [{ id: 'r1', roomName: ROOM, position: [0, 0, 0], rotationY: 0 }],
        spawnPoints: [],
      },
    },
  ],
};

const PRIMARY = 'main canvas';
// Previews set no ready flag: their drawn pixels are the signal.
const PREVIEW = 'main canvas[data-officexr-view]';

async function openRenderPage(page: Page) {
  await page.addInitScript((seed) => {
    (window as unknown as { __OFFICEXR_TEST_SEED__: unknown }).__OFFICEXR_TEST_SEED__ = seed;
  }, SEED);
  await page.goto('/?test=1&op=render&w=400&h=300&target=layout&name=default&camera=iso', { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => (window as { __officexrRenderReady?: unknown }).__officexrRenderReady, null, {
    timeout: 200_000,
  });
}

const inMode = (mode: Mode, ready: 'bevy' | 'none') => async (page: Page) => {
  await goToModeHermetic(page, mode, SEED);
  if (ready === 'bevy') {
    await waitForBevyReady(page);
  }
};

const SURFACES: Array<{ surface: string; open: (page: Page) => Promise<void>; canvas: string }> = [
  { surface: 'room view (Debug mode)', open: inMode('debug', 'bevy'), canvas: PRIMARY },
  { surface: 'room editor', open: inMode('room', 'bevy'), canvas: PRIMARY },
  { surface: 'layout editor', open: inMode('layout', 'bevy'), canvas: PRIMARY },
  { surface: 'map editor', open: inMode('map', 'bevy'), canvas: PRIMARY },
  { surface: 'mugshot', open: inMode('mugshot', 'bevy'), canvas: PRIMARY },
  { surface: 'object editor preview', open: inMode('object', 'none'), canvas: PREVIEW },
  { surface: 'character preview', open: inMode('character', 'none'), canvas: PREVIEW },
  { surface: '?op=render', open: openRenderPage, canvas: '[data-render-target]' },
];

for (const { surface, open, canvas } of SURFACES) {
  test(`${surface} draws through Bevy`, async ({ page }) => {
    test.setTimeout(240_000);
    await forceWebgl2(page);
    await open(page);
    await expect(page.locator(canvas)).toBeVisible({ timeout: 60_000 });
    const colours = async () => new Set((await canvasPixels(page, [canvas]))[0]).size;
    await expect.poll(colours, { timeout: 120_000, intervals: [2_000] }).toBeGreaterThan(16);
  });
}
