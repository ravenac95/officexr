/**
 * Every studio surface boots Bevy and draws a non-blank canvas, hermetically (`?test=1`). What
 * they draw is the native harness's (`crates/officexr-render/tests/visual/`). The web app's room
 * view is the same `PlayView` as Debug mode's.
 */

import { expect, test, type Page } from '@playwright/test';
import { canvasPixels, forceWebgl2, goToModeHermetic, pageErrors, waitForBevyReady, type Mode } from './helpers.ts';

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

// Each wait's worst case, which the test's timeout covers in full.
const READY_MS = 170_000;
const VISIBLE_MS = 60_000;
const DRAWN_MS = 60_000;

async function openRenderPage(page: Page, errors: Promise<never>) {
  await page.addInitScript((seed) => {
    (window as unknown as { __OFFICEXR_TEST_SEED__: unknown }).__OFFICEXR_TEST_SEED__ = seed;
  }, SEED);
  await page.goto('/?test=1&op=render&w=400&h=300&target=layout&name=default&camera=iso', { waitUntil: 'domcontentloaded' });
  const settled = page.waitForFunction(
    () => {
      const w = window as { __officexrRenderReady?: unknown; __officexrRenderError?: string };
      return w.__officexrRenderReady ? 'ready' : (w.__officexrRenderError ?? null);
    },
    null,
    { timeout: READY_MS },
  );
  // The error sentinel, if set, is what this prints instead of 'ready'.
  expect(await (await Promise.race([settled, errors])).jsonValue()).toBe('ready');
}

const inMode = (mode: Mode, ready: 'bevy' | 'none') => async (page: Page, errors: Promise<never>) => {
  await goToModeHermetic(page, mode, SEED);
  if (ready === 'bevy') {
    await waitForBevyReady(page, errors, READY_MS);
  }
};

/**
 * The fraction of `selector`'s pixels that differ from its commonest colour (a backdrop or clear
 * colour), with the rest of the page hidden so HUD text and toolbars over the canvas don't count.
 */
async function drawnFraction(page: Page, selector: string): Promise<number> {
  const onlyCanvas = await page.addStyleTag({
    content: `body * { visibility: hidden !important; } ${selector}, ${selector} * { visibility: visible !important; }`,
  });
  try {
    const [pixels] = await canvasPixels(page, [selector]);
    const counts = new Map<number, number>();
    for (const p of pixels!) {
      counts.set(p, (counts.get(p) ?? 0) + 1);
    }
    return 1 - Math.max(...counts.values()) / pixels!.length;
  } finally {
    await onlyCanvas.evaluate((style) => (style as Element).remove());
  }
}

const SURFACES: Array<{ surface: string; open: (page: Page, errors: Promise<never>) => Promise<void>; canvas: string }> = [
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
    test.setTimeout(READY_MS + VISIBLE_MS + DRAWN_MS + 30_000);
    await forceWebgl2(page);
    const errors = pageErrors(page);
    await open(page, errors);
    await expect(page.locator(canvas)).toBeVisible({ timeout: VISIBLE_MS });
    await Promise.race([expect.poll(() => drawnFraction(page, canvas), { timeout: DRAWN_MS, intervals: [2_000] }).toBeGreaterThan(0.02), errors]);
  });
}
