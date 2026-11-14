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
  const catalog = validateWorldObjectKindCatalog(JSON.parse(await fs.readFile(CATALOG_PATH, 'utf-8')));
  const io = await registerCompressionCodecs(new NodeIO());

  let measured = 0;
  const failed: string[] = [];
  const kinds = [];
  for (const kind of catalog.kinds) {
    if (kind.category === 'character') {
      kinds.push(kind);
      continue;
    }
    try {
      const { localAABB, ...size } = measureKindDocument(
        await io.read(path.resolve(STUDIO_PUBLIC, kind.gltfPath.replace(/^\//, ''))),
        kind.scale,
      );
      kinds.push({ ...kind, dimensions: size, localAABB });
      measured++;
    } catch (err) {
      console.warn(`[bake-dimensions] ${kind.id} failed:`, err instanceof Error ? err.message : err);
      failed.push(kind.id);
      kinds.push(kind);
    }
  }

  console.log(`[bake-dimensions] Measured ${measured} kinds; ${failed.length} failed.`);
  if (measured === 0) {
    console.error('[bake-dimensions] No kinds measured; refusing to overwrite catalog.');
    process.exit(1);
  }
  await fs.writeFile(CATALOG_PATH, JSON.stringify({ ...catalog, updatedAt: Date.now(), kinds }));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
