#!/usr/bin/env tsx
/**
 * Compiles a map into the scene JSON `crates/officexr-render` reads: the
 * catalog kinds its rooms place, its baked layout placements, and the default
 * world lighting and background (`--editor-lighting`: the studio render page's
 * editor preset and flat background instead).
 *
 * Bevy's glTF loader reads neither meshopt nor quantized meshes, so each baked
 * layout (`bake:layout`) is decoded into `packages/studio/public/baked-layouts/`.
 *
 * Usage: pnpm --filter @officexr/world export:render-scene <mapName> <outFile> [--editor-lighting]
 */

import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO } from '@gltf-transform/core';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import { decodeCompressedGlb, registerCompressionCodecs } from '../src/app/glb-compression.ts';
import { createCatalogService } from '../src/app/catalog-service.ts';
import { DEFAULT_EDITOR_LIGHTING } from '../src/app/editor-lighting.ts';
import { createInstanceGeometry } from '../src/app/geometry-service.ts';
import { loadMap } from '../src/app/map-loader.ts';
import { createRoomService } from '../src/app/room-service.ts';
import { VOXEL_SIZE } from '../src/renderer/config.ts';
import { InMemoryLayoutStorage } from '../src/scenes/in-memory-layout-storage.ts';
import { InMemoryMapStorage } from '../src/scenes/in-memory-map-storage.ts';
import { InMemoryRoomStorage } from '../src/scenes/in-memory-room-storage.ts';
import { validateWorldObjectKindCatalog } from '../src/scenes/world-object-kinds-schema.ts';

const WORLD_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const STUDIO_PUBLIC = path.resolve(WORLD_ROOT, '../studio/public');
const EDITOR_BACKGROUND = '#1b1d22';

async function decodeBakedLayout(io: NodeIO, name: string): Promise<string> {
  const baked = path.resolve(WORLD_ROOT, 'baked-layouts', `${name}.glb`);
  const glb = await fs.readFile(baked).catch((err: unknown) => {
    throw new Error(`cannot read ${baked}; run \`pnpm --filter @officexr/world bake:layout ${name}\``, { cause: err });
  });
  const decoded = await decodeCompressedGlb(io, glb);
  const gltfPath = `/baked-layouts/${name}.glb`;
  await fs.mkdir(path.resolve(STUDIO_PUBLIC, 'baked-layouts'), { recursive: true });
  await fs.writeFile(path.resolve(STUDIO_PUBLIC, gltfPath.slice(1)), decoded);
  return gltfPath;
}

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

async function main(mapName: string, outFile: string, editorLighting: boolean): Promise<void> {
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
  const roomService = createRoomService({ geometry });
  const { mapDoc, rooms, bakedLayouts } = await loadMap({ storages, catalog, roomService, geometry }, mapName);
  // As in the studio's map view: rooms draw their own commands, layouts draw from their bake.
  const objects = roomService.compileMap(mapDoc, rooms);
  const io = await registerCompressionCodecs(new NodeIO());
  const gltfPaths = new Map<string, string>();
  for (const name of new Set(bakedLayouts.map((p) => p.layoutName))) {
    gltfPaths.set(name, await decodeBakedLayout(io, name));
  }

  const usedKindIds = new Set(objects.instances.map((i) => i.kindId));
  const scene = {
    kinds: catalog.getCatalog().kinds.filter((k) => usedKindIds.has(k.id)),
    objects,
    lighting: editorLighting ? DEFAULT_EDITOR_LIGHTING : DEFAULT_WORLD_CONSTANTS.lighting,
    background: DEFAULT_WORLD_CONSTANTS.background,
    clearColor: editorLighting ? EDITOR_BACKGROUND : undefined,
    bakedLayouts: bakedLayouts.map((p) => ({
      gltfPath: gltfPaths.get(p.layoutName),
      position: p.position,
      rotationY: p.rotationY,
    })),
  };
  await fs.writeFile(path.resolve(outFile), `${JSON.stringify(scene, null, 2)}\n`);
}

const [mapName, outFile, flag] = process.argv.slice(2);
if (!mapName || !outFile || (flag !== undefined && flag !== '--editor-lighting')) {
  console.error('usage: export-render-scene.ts <mapName> <outFile> [--editor-lighting]');
  process.exit(1);
}
await main(mapName, outFile, flag === '--editor-lighting');
