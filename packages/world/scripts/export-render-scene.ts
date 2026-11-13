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
import { createCatalogService } from '../src/app/catalog-service.ts';
import { createInstanceGeometry } from '../src/app/geometry-service.ts';
import { loadMap } from '../src/app/map-loader.ts';
import { createRoomService } from '../src/app/room-service.ts';
import { VOXEL_SIZE } from '../src/renderer/config.ts';
import { InMemoryLayoutStorage } from '../src/scenes/in-memory-layout-storage.ts';
import { InMemoryMapStorage } from '../src/scenes/in-memory-map-storage.ts';
import { InMemoryRoomStorage } from '../src/scenes/in-memory-room-storage.ts';
import { validateWorldObjectKindCatalog } from '../src/scenes/world-object-kinds-schema.ts';

const WORLD_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** Every `<dir>/*.json` as a storage seed entry. */
async function seed<T>(dir: string): Promise<{ name: string; doc: T }[]> {
  const files = (await fs.readdir(path.resolve(WORLD_ROOT, dir))).filter((f) => f.endsWith('.json'));
  return Promise.all(
    files.map(async (f) => ({
      name: path.basename(f, '.json'),
      doc: JSON.parse(await fs.readFile(path.resolve(WORLD_ROOT, dir, f), 'utf-8')) as T,
    })),
  );
}

async function main(mapName: string, outFile: string): Promise<void> {
  const catalog = createCatalogService({
    defaultCatalog: validateWorldObjectKindCatalog(
      JSON.parse(await fs.readFile(path.resolve(WORLD_ROOT, 'world-object-kinds.json'), 'utf-8')) as unknown,
    ),
    apiPath: '',
  });
  const geometry = createInstanceGeometry({ catalog, voxelSize: VOXEL_SIZE });
  const storages = {
    maps: new InMemoryMapStorage({ seed: await seed('maps') }),
    rooms: new InMemoryRoomStorage({ seed: await seed('rooms') }),
    layouts: new InMemoryLayoutStorage({ seed: await seed('layouts') }),
  };
  const { worldObjects: objects } = await loadMap(
    { storages, catalog, roomService: createRoomService({ geometry }), geometry },
    mapName,
  );

  const usedKindIds = new Set(objects.instances.map((i) => i.kindId));
  const scene = {
    kinds: catalog.getCatalog().kinds.filter((k) => usedKindIds.has(k.id)),
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
