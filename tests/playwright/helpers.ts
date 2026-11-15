/**
 * Shared Playwright helpers for the studio regression suite.
 */

import { expect, type Page } from '@playwright/test';
import { PNG } from 'pngjs';

/** Hides WebGPU so `loadRenderer` picks the WebGL2 build, the one SwiftShader renders reliably. */
export async function forceWebgl2(page: Page) {
  await page.addInitScript(() => Object.defineProperty(Navigator.prototype, 'gpu', { get: () => undefined }));
}

/**
 * Waits until the page's Bevy view has loaded every model (`__officexrBevyReady`), failing
 * fast on a page error such as a wasm panic. Boots slowly under SwiftShader, hence the long timeout.
 */
export async function waitForBevyReady(page: Page, timeout = 170_000): Promise<void> {
  const pageError = new Promise<never>((_, reject) => page.once('pageerror', reject));
  const ready = page.waitForFunction(
    () => (window as { __officexrBevyReady?: boolean }).__officexrBevyReady === true,
    null,
    { timeout },
  );
  await Promise.race([ready, pageError]);
}

export type Mode = 'map' | 'room' | 'layout' | 'object' | 'character' | 'debug' | 'mugshot' | 'sounds';

/** Navigate to a mode by hash. The studio mirrors `#<mode>` ↔ state.
 *
 * Uses `waitUntil: 'domcontentloaded'` because:
 *   - `'networkidle'` never fires (Debug opens a long-lived WebSocket).
 *   - `'load'` is gated on every transitive sub-resource (heavy GLTFs,
 *     wasm) finishing.
 *
 * The active-tab assertion below is the real "mode mounted" gate. */
export async function goToMode(
  page: Page,
  mode: Mode,
): Promise<void> {
  await page.goto(`/#${mode}`, { waitUntil: 'domcontentloaded' });
  await expect(
    page.locator(`button[role="tab"][aria-selected="true"]`),
  ).toHaveText(modeLabel(mode), { timeout: 8_000 });
}

/**
 * Navigate to a mode in hermetic test mode (`?test=1`): the studio
 * boots with a bundled catalog and in-memory storages instead of the
 * dev-server backend. Optionally seeds those storages via
 * `window.__OFFICEXR_TEST_SEED__` (injected before load) so an editor
 * opens with known content.
 *
 * `seed` must be JSON-serializable (it crosses into the page via
 * `addInitScript`).
 */
export async function goToModeHermetic(
  page: Page,
  mode: Mode,
  seed?: unknown,
): Promise<void> {
  if (seed !== undefined) {
    await page.addInitScript((s) => {
      (window as unknown as { __OFFICEXR_TEST_SEED__: unknown }).__OFFICEXR_TEST_SEED__ = s;
    }, seed);
  }
  await page.goto(`/?test=1#${mode}`, { waitUntil: 'domcontentloaded' });
  await expect(
    page.locator(`button[role="tab"][aria-selected="true"]`),
  ).toHaveText(modeLabel(mode), { timeout: 8_000 });
}

/** A studio tab's label is its mode's name, capitalised. */
function modeLabel(mode: Mode): string {
  return mode[0]!.toUpperCase() + mode.slice(1);
}

/**
 * Debug mode on `mapName`, through the studio's remembered last map (the picker is a Radix select,
 * awkward to drive before the page settles). `sim` picks the `?sim=` simulation.
 */
export async function goToDebugWithMap(page: Page, mapName: string, sim?: 'rust'): Promise<void> {
  await page.addInitScript((m: string) => {
    try {
      localStorage.setItem('officexr:studio:lastMap', m);
    } catch {
      // about:blank or private mode
    }
  }, mapName);
  await page.goto(`/${sim ? `?sim=${sim}` : ''}#debug`, { waitUntil: 'domcontentloaded' });
}

/**
 * Wait until the page has a visible `<canvas>`. Returns the canvas locator.
 *
 * `visibleTimeoutMs` covers cold-Vite-cache first-page-load lag.
 */
export async function waitForCanvasReady(
  page: Page,
  settleMs = 1500,
  visibleTimeoutMs = 20_000,
) {
  const canvas = page.locator('canvas').first();
  await expect(canvas).toBeVisible({ timeout: visibleTimeoutMs });
  await page.waitForTimeout(settleMs);
  return canvas;
}

/**
 * Each canvas's pixels from one page screenshot, which sees WebGL and read-back canvases alike as
 * composited. Element screenshots wait on a compositor frame, too slow under SwiftShader. `fromTop`
 * skips that fraction of each canvas's height.
 */
export async function canvasPixels(page: Page, selectors: string[], fromTop = 0) {
  const boxes = await page.evaluate(
    (all) => all.map((s) => document.querySelector(s)!.getBoundingClientRect().toJSON() as DOMRect),
    selectors,
  );
  const shot = PNG.sync.read(await page.screenshot({ timeout: 120_000 }));
  return boxes.map((box) => {
    const out: number[] = [];
    for (let y = Math.ceil(box.y + box.height * fromTop); y < Math.floor(box.y + box.height); y++) {
      for (let x = Math.ceil(box.x); x < Math.floor(box.x + box.width); x++) {
        out.push(shot.data.readUInt32BE((y * shot.width + x) * 4));
      }
    }
    return out;
  });
}
