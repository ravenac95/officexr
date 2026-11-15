/**
 * `?op=render`: draws a stored map / room / layout from a requested camera
 * and signals completion on `window.__officexrRenderReady` (or
 * `__officexrRenderError`). Runs hermetically (`?test=1`) against a seeded
 * map; baked GLBs are served by route interception. That it draws is
 * `rendering-smoke.spec.ts`'s.
 */

import { expect, test, type Page } from '@playwright/test';
import { forceWebgl2 } from './helpers.ts';
import { boxGlb } from './lib/box-glb.ts';

const blueBlock = (id: string, position: number[]) => ({
  id,
  op: 'placeObject',
  kindId: '__primitive_blue',
  position,
});

const room = (name: string, extra: object) => ({
  name,
  doc: { schemaVersion: 5, name, title: name, updatedAt: 0, commands: [], groups: {}, ...extra },
});

// A real catalog kind (BlockBits, 2 m), so the page has to load a model GLB.
const catalogBlock = (id: string, position: number[]) => ({
  id,
  op: 'placeObject',
  kindId: 'colored_block_blue',
  position,
});

const layout = (name: string, position = [0, 0, 0]) => ({
  name,
  doc: { schemaVersion: 1, name, updatedAt: 0, commands: [blueBlock('l1', position)], groups: {} },
});

const map = (name: string, rooms: object[]) => ({
  name,
  doc: { schemaVersion: 2, name, title: name, updatedAt: 0, rooms, spawnPoints: [] },
});

// `wing` is baked-only (its geometry is a green GLB) and placed twice; `hall`
// is two blue catalog cubes in a row along X, placed once, rotated a quarter
// turn so the row runs along Z. `annex` has commands AND a layout, its bake
// drawn 4 m along X from them. The `ghost` layout has no GLB.
const SEED = {
  maps: [
    map('office', [
      { id: 'wing-a', roomName: 'wing', position: [0, 0, 0], rotationY: 0 },
      { id: 'wing-b', roomName: 'wing', position: [12, 0, 0], rotationY: 0 },
      { id: 'hall-1', roomName: 'hall', position: [0, 0, 12], rotationY: 1 },
    ]),
    map('annexed', [{ id: 'annex-1', roomName: 'annex', position: [0, 0, 0], rotationY: 0 }]),
    map('haunted', [{ id: 'ghost-1', roomName: 'ghost', position: [0, 0, 0], rotationY: 0 }]),
  ],
  rooms: [
    room('wing', { layoutName: 'wing-layout' }),
    room('hall', { commands: [catalogBlock('c1', [0, 0, 0]), catalogBlock('c2', [4, 0, 0])] }),
    room('annex', { commands: [blueBlock('c1', [0, 0, 0])], layoutName: 'annex-layout' }),
    room('ghost', { layoutName: 'ghost-layout' }),
  ],
  layouts: [layout('wing-layout'), layout('annex-layout', [8, 0, 0]), layout('ghost-layout')],
};

type RenderReady = { bounds: { min: number[]; max: number[] }; metresPerPixel?: number };

test.setTimeout(240_000);

test.beforeEach(async ({ page }) => {
  await forceWebgl2(page);
  await page.addInitScript((seed) => {
    (window as unknown as { __OFFICEXR_TEST_SEED__: unknown }).__OFFICEXR_TEST_SEED__ = seed;
  }, SEED);
  const greenBox = Buffer.from(await boxGlb([2, 2, 2], [0.1, 0.9, 0.1]));
  await page.route(/\/api\/baked-layouts\/(wing|annex)-layout\.glb$/, (route) =>
    route.fulfill({ body: greenBox, contentType: 'model/gltf-binary' }),
  );
  await page.route('**/api/baked-layouts/ghost-layout.glb', (route) => route.fulfill({ status: 404, body: 'no bake' }));
});

async function render(page: Page, query: string) {
  await page.goto(`/?test=1&op=render&w=400&h=300&${query}`, { waitUntil: 'domcontentloaded' });
  const settled = await page.waitForFunction(
    () => {
      const w = window as unknown as { __officexrRenderReady?: unknown; __officexrRenderError?: string };
      return w.__officexrRenderReady ? 'ready' : w.__officexrRenderError ? 'error' : null;
    },
    null,
    { timeout: 200_000 },
  );
  return (await settled.jsonValue()) as 'ready' | 'error';
}

const readySentinel = (page: Page) =>
  page.evaluate(() => (window as unknown as { __officexrRenderReady: RenderReady }).__officexrRenderReady);
const errorSentinel = (page: Page) =>
  page.evaluate(() => (window as unknown as { __officexrRenderError?: string }).__officexrRenderError);

test('reports the framed bounds and scale for the top camera', async ({ page }) => {
  expect(await render(page, 'target=map&name=office&camera=top')).toBe('ready');
  const { bounds, metresPerPixel } = await readySentinel(page);
  // wing-a x 0..2, wing-b x 6..8 (voxels 12 at 0.5 m), hall rotated into z 2..6.
  expect(bounds.min[0]).toBeCloseTo(0);
  expect(bounds.max[0]).toBeCloseTo(8);
  // z 0..6 plus a 1 m margin each side is the tighter axis: 8 m over 300 px.
  expect(metresPerPixel).toBeCloseTo(8 / 300, 6);
});

test('bounds include the baked layout of a room that also has commands', async ({ page }) => {
  expect(await render(page, 'target=map&name=annexed&camera=top')).toBe('ready');
  const { bounds } = await readySentinel(page);
  // The room's own cube is x 0..2; its layout's cube is x 4..6.
  expect(bounds.max[0]).toBeCloseTo(6);
});

test('an unknown document sets the error sentinel', async ({ page }) => {
  expect(await render(page, 'target=map&name=nowhere')).toBe('error');
  expect(await errorSentinel(page)).toContain('nowhere');
});

test('bad parameters set the error sentinel', async ({ page }) => {
  expect(await render(page, 'target=building&name=office')).toBe('error');
  expect(await errorSentinel(page)).toContain('target');
});

test('a missing baked GLB sets the error sentinel instead of ready', async ({ page }) => {
  expect(await render(page, 'target=map&name=haunted&camera=top')).toBe('error');
  expect(await errorSentinel(page)).toContain('ghost-layout');
  // Give the frame counter time to (wrongly) fire.
  await page.waitForTimeout(500);
  expect(await page.evaluate(() => (window as unknown as { __officexrRenderReady?: unknown }).__officexrRenderReady)).toBeUndefined();
});
