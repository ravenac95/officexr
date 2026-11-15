/**
 * Mugshot smoke test: each character loads in the Bevy mugshot view on the 2×2 cube square,
 * the ready flag is set, and its feet settled on the cube tops. Placement and pixels are the
 * native harness's (`crates/officexr-render/tests/visual/`), which asserts the same 10 cm contract.
 */
import { test, expect } from '@playwright/test';
import { canvasPixels, forceWebgl2, waitForBevyReady } from './helpers.ts';

const CHARACTERS = ['Barbarian', 'Knight', 'Mage', 'Ranger', 'Rogue', 'Rogue_Hooded'] as const;

/** The cube tops. */
const SURFACE_Y = 1;
const MAX_GROUND_SINK = 0.1;

for (const character of CHARACTERS) {
  test(`mugshot: ${character} loads and stands on the cubes`, async ({ page }) => {
    test.setTimeout(240_000);
    await forceWebgl2(page);
    await page.goto(`/#mugshot/${character}`, { waitUntil: 'domcontentloaded' });
    await waitForBevyReady(page);

    const feetY = await page.evaluate(() => (window as { __officexrBevyMugshotFeetY?: number }).__officexrBevyMugshotFeetY);
    expect(Math.abs((feetY ?? NaN) - SURFACE_Y)).toBeLessThan(MAX_GROUND_SINK);

    const [pixels] = await canvasPixels(page, ['main canvas']);
    expect(new Set(pixels).size).toBeGreaterThan(50);
  });
}
