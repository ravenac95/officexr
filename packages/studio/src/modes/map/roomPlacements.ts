import type { WorldDiff } from '@officexr/render-wasm';
import type { RoomService } from '@officexr/world/app';
import type { MapDocumentV2, RoomDocument } from '@officexr/world/scenes';

export type RoomPlacements = Required<Pick<WorldDiff, 'bakedLayouts'>> & {
  /** The rooms' own objects, placed and turned; a linked layout's are drawn by its baked GLB instead. */
  instances: ReturnType<RoomService['compileMap']>['instances'];
};

/**
 * What a map's placed rooms draw in a Bevy world view: each room's compiled objects, and a baked
 * layout per placement of a room linked to one. `layoutUrl` resolves a layout's GLB; it must end in
 * `.glb`, which Bevy picks the loader by.
 */
export function roomPlacements(
  doc: MapDocumentV2,
  rooms: ReadonlyMap<string, RoomDocument>,
  roomService: Pick<RoomService, 'compileMap'>,
  layoutUrl: (layoutName: string) => string,
): RoomPlacements {
  return {
    instances: roomService.compileMap(doc, rooms).instances,
    bakedLayouts: doc.rooms.flatMap((ri) => {
      const layoutName = rooms.get(ri.roomName)?.layoutName;
      return layoutName
        ? [
            {
              gltfPath: layoutUrl(layoutName),
              position: [...ri.position],
              rotationY: ri.rotationY ?? 0,
              layoutName,
              roomInstanceId: ri.id,
            },
          ]
        : [];
    }),
  };
}
