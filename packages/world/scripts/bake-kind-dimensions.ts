#!/usr/bin/env tsx
/**
 * Measures every non-character kind's model and writes `dimensions` and
 * `localAABB` into `world-object-kinds.json`. Models resolve against
 * `packages/studio/public`.
 *
 * Usage: pnpm --filter @officexr/world bake:dimensions
 */

import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO } from '@gltf-transform/core';
import { registerCompressionCodecs } from '../src/app/glb-compression.ts';
import { measureKindDocument } from '../src/app/kind-dimensions.ts';
import { validateWorldObjectKindCatalog } from '../src/scenes/world-object-kinds-schema.ts';

const WORLD_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const STUDIO_PUBLIC = path.resolve(WORLD_ROOT, '../studio/public');
const CATALOG_PATH = path.join(WORLD_ROOT, 'world-object-kinds.json');

async function main() {
  const raw = JSON.parse(await fs.readFile(CATALOG_PATH, 'utf-8')) as { updatedAt: number; kinds: Record<string, unknown>[] };
  validateWorldObjectKindCatalog(raw);
  const io = await registerCompressionCodecs(new NodeIO());

  let measured = 0;
  let failed = 0;
  for (const kind of raw.kinds) {
    if (kind.category === 'character') {
      continue;
    }
    try {
      const { localAABB, ...dimensions } = measureKindDocument(
        await io.read(path.resolve(STUDIO_PUBLIC, String(kind.gltfPath).replace(/^\//, ''))),
        Number(kind.scale),
      );
      Object.assign(kind, { dimensions, localAABB });
      measured++;
    } catch {
      failed++;
    }
  }

  console.log(`[bake-dimensions] Measured ${measured} kinds; ${failed} failed (model missing or unreadable).`);
  if (measured === 0) {
    console.error('[bake-dimensions] No kinds measured; refusing to overwrite catalog.');
    process.exit(1);
  }
  raw.updatedAt = Date.now();
  await fs.writeFile(CATALOG_PATH, JSON.stringify(raw));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
