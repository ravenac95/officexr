#!/usr/bin/env tsx
/**
 * Measures every non-character kind's model and writes `dimensions` and
 * `localAABB` into `world-object-kinds.json`, except for kinds that set
 * `dimensionsPinned`; those are only checked for drift. Models resolve against
 * `packages/studio/public`.
 *
 * Usage: pnpm --filter @officexr/world bake:dimensions
 */

import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO } from '@gltf-transform/core';
import { registerCompressionCodecs } from '../src/app/glb-compression.ts';
import { measureKindDocument, mergeKindMeasurement } from '../src/app/kind-dimensions.ts';
import { validateWorldObjectKindCatalog } from '../src/scenes/world-object-kinds-schema.ts';

const WORLD_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const STUDIO_PUBLIC = path.resolve(WORLD_ROOT, '../studio/public');
const CATALOG_PATH = path.join(WORLD_ROOT, 'world-object-kinds.json');

async function main() {
  const raw = JSON.parse(await fs.readFile(CATALOG_PATH, 'utf-8')) as { updatedAt: number; kinds: Record<string, unknown>[] };
  validateWorldObjectKindCatalog(raw);
  const before = JSON.stringify(raw.kinds);
  const io = await registerCompressionCodecs(new NodeIO());

  let measured = 0;
  let failed = 0;
  const drifted: string[] = [];
  for (const kind of raw.kinds) {
    if (kind.category === 'character') {
      continue;
    }
    try {
      const measurement = measureKindDocument(
        await io.read(path.resolve(STUDIO_PUBLIC, String(kind.gltfPath).replace(/^\//, ''))),
        Number(kind.scale),
      );
      if (mergeKindMeasurement(kind, measurement).pinnedDrift) {
        drifted.push(`${String(kind.id)} (measured ${measurement.width}x${measurement.height}x${measurement.depth})`);
      }
      measured++;
    } catch {
      failed++;
    }
  }

  console.log(`[bake-dimensions] Measured ${measured} kinds; ${failed} failed (model missing or unreadable).`);
  if (drifted.length > 0) {
    console.log(`[bake-dimensions] Pinned drift, left unchanged: ${drifted.join(', ')}.`);
  }
  if (measured === 0) {
    console.error('[bake-dimensions] No kinds measured; refusing to overwrite catalog.');
    process.exit(1);
  }
  if (JSON.stringify(raw.kinds) !== before) {
    raw.updatedAt = Date.now();
    await fs.writeFile(CATALOG_PATH, JSON.stringify(raw));
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
