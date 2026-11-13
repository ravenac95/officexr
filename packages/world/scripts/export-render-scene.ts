#!/usr/bin/env tsx
/**
 * Compiles a map into the scene JSON `crates/officexr-render` reads: the
 * catalog kinds it uses, its `WorldObjects`, and the default world lighting
 * and background.
 *
 * Usage: pnpm --filter @officexr/world export:render-scene <mapName> <outFile>
 */

import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import { createInstanceGeometry } from '../src/app/geometry-service.ts';
import { createRoomService } from '../src/app/room-service.ts';
import { VOXEL_SIZE } from '../src/renderer/config.ts';
import { deserializeLayout, type LayoutDocument } from '../src/scenes/layout-document.ts';
import { loadAll, mapLayoutPlacements } from '../src/scenes/map-layout-placements.ts';
import { deserializeMap, deserializeScene, migrateToV5 } from '../src/scenes/serialize.ts';
import { validateWorldObjectKindCatalog } from '../src/scenes/world-object-kinds-schema.ts';

const WORLD_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function readJson(file: string): Promise<unknown> {
  return JSON.parse(await fs.readFile(path.resolve(WORLD_ROOT, file), 'utf-8')) as unknown;
}

async function main(mapName: string, outFile: string): Promise<void> {
  const catalog = validateWorldObjectKindCatalog(await readJson('world-object-kinds.json'));
  const kindById = new Map(catalog.kinds.map((k) => [k.id, k]));
  const geometry = createInstanceGeometry({ catalog: { getKind: (id) => kindById.get(id) }, voxelSize: VOXEL_SIZE });

  const { mapDoc, rooms, placements } = await mapLayoutPlacements(
    deserializeMap(await readJson(`maps/${mapName}.json`)),
    async (name) => migrateToV5(deserializeScene(await readJson(`rooms/${name}.json`))),
  );
  const layouts = await loadAll<LayoutDocument>(
    new Set(placements.map((p) => p.layoutName)),
    async (name) => deserializeLayout(await readJson(`layouts/${name}.json`)),
    'layout',
  );
  const objects = createRoomService({ geometry }).compileMap(mapDoc, rooms, layouts);

  const usedKindIds = new Set(objects.instances.map((i) => i.kindId));
  const scene = {
    kinds: catalog.kinds.filter((k) => usedKindIds.has(k.id)),
    objects,
    lighting: DEFAULT_WORLD_CONSTANTS.lighting,
    background: DEFAULT_WORLD_CONSTANTS.background,
  };
  await fs.writeFile(path.resolve(outFile), `${JSON.stringify(scene, null, 2)}\n`);
}

const [mapName, outFile] = process.argv.slice(2);
if (!mapName || !outFile) {
  console.error('usage: export-render-scene.ts <mapName> <outFile>');
  process.exit(1);
}
await main(mapName, outFile);
