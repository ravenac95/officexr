import type { WorldObjects } from '@officexr/sdk';
import { bakedLayoutUrl, loadMap, modelUrl, type ApplicationApi, type MapLoaderDeps } from '@officexr/world/app';
import { paths, studioApiUrl } from '../../apiOrigin.ts';
import { roomPlacements, type RoomPlacements } from '../map/roomPlacements.ts';
import { unionBounds, type Bounds } from './renderFraming.ts';
import type { RenderParams } from './renderParams.ts';

export type RenderStorages = MapLoaderDeps['storages'];

// ponytail: baked layouts load without credentials; fine while headless render only runs same-origin.
/** A baked layout's GLB, absolute (Bevy resolves relative paths against the model origin) and `.glb` (Bevy picks loaders by extension). */
export const layoutUrl = (layoutName: string) =>
  new URL(`${bakedLayoutUrl(layoutName, studioApiUrl(paths.bakedLayouts))}.glb`, window.location.href).href;

/** What the render page draws; `bounds` is what the camera frames. */
export type RenderContent = { bounds: Bounds; modelUrls: string[] } & (
  | { type: 'map'; placed: RoomPlacements }
  | { type: 'room'; worldObjects: WorldObjects; layoutName?: string }
  | { type: 'layout'; worldObjects: WorldObjects }
);

function nothingToRender({ target, name }: RenderParams): Error {
  return new Error(`${target} "${name}" has nothing to render`);
}

export async function loadRenderContent(
  api: ApplicationApi,
  storages: RenderStorages,
  params: RenderParams,
): Promise<RenderContent> {
  const { target, name } = params;
  const boundsOf = (instances: WorldObjects['instances']) =>
    unionBounds(instances, (inst) => api.geometry.worldAABBOfInstance(inst));
  /** Catalog kinds only: primitives have no model to load. */
  const modelUrlsOf = (instances: WorldObjects['instances']) => [
    ...new Set(
      instances.flatMap((inst) => {
        const kind = api.catalog.getKind(inst.kindId);
        return kind ? [modelUrl(kind.gltfPath)] : [];
      }),
    ),
  ];

  if (target === 'map') {
    const map = await loadMap(
      { storages, catalog: api.catalog, roomService: api.rooms, geometry: api.geometry },
      name,
    );
    // loadMap's compile already holds every linked layout's instances, which bound the baked GLBs.
    const bounds = boundsOf(map.worldObjects.instances);
    if (!bounds) {
      throw nothingToRender(params);
    }
    // Layout instances aren't drawn as models (their GLB is), so preload only the rooms' own kinds.
    const placed = roomPlacements(map.mapDoc, map.rooms, api.rooms, layoutUrl);
    return { type: 'map', bounds, modelUrls: modelUrlsOf(placed.instances), placed };
  }

  // Kind strides are only correct once the catalog has landed.
  await api.catalog.ready();
  if (target === 'layout') {
    const layout = await storages.layouts.load(name);
    if (!layout) {
      throw new Error(`layout "${name}" not found`);
    }
    const worldObjects = api.rooms.compileScene(layout);
    const bounds = boundsOf(worldObjects.instances);
    if (!bounds) {
      throw nothingToRender(params);
    }
    return { type: 'layout', bounds, modelUrls: modelUrlsOf(worldObjects.instances), worldObjects };
  }

  const room = await storages.rooms.load(name);
  if (!room) {
    throw new Error(`room "${name}" not found`);
  }
  const worldObjects = api.rooms.compileScene(room);
  // The baked GLB has no bounds of its own; its source layout's commands stand in for them.
  const layout = room.layoutName ? await storages.layouts.load(room.layoutName) : null;
  const layoutInstances = layout ? api.rooms.compileScene(layout).instances : [];
  const bounds = boundsOf([...worldObjects.instances, ...layoutInstances]);
  if (!bounds) {
    throw nothingToRender(params);
  }
  return {
    type: 'room',
    bounds,
    modelUrls: modelUrlsOf(worldObjects.instances),
    worldObjects,
    layoutName: room.layoutName,
  };
}
