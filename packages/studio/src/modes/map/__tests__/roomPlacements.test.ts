import { describe, expect, it } from 'vitest';
import { emptyMapDocument, emptyRoomDocument, newPlaceObject } from '@officexr/world/scenes';
import { createTestApi } from '../../../test-harness/createTestApi.ts';
import { roomPlacements } from '../roomPlacements.ts';

describe('roomPlacements', () => {
  const api = createTestApi();
  const built = emptyRoomDocument('built');
  built.commands = [newPlaceObject({ kindId: '__primitive_blue', position: [0, 0, 0] })];
  const baked = emptyRoomDocument('baked');
  baked.layoutName = 'wing';
  const map = {
    ...emptyMapDocument('m'),
    rooms: [
      { id: 'a', roomName: 'baked', position: [0, 0, 0] as [number, number, number], rotationY: 0 as const },
      { id: 'b', roomName: 'baked', position: [12, 0, 0] as [number, number, number], rotationY: 1 as const },
      { id: 'c', roomName: 'built', position: [0, 0, 12] as [number, number, number], rotationY: 0 as const },
      { id: 'd', roomName: 'missing', position: [0, 0, 24] as [number, number, number], rotationY: 0 as const },
    ],
  };
  const rooms = new Map([
    ['built', built],
    ['baked', baked],
  ]);
  const placed = roomPlacements(map, rooms, api.rooms, (name) => `/layouts/${name}.glb`);

  it('places a baked layout per placement of a linked room, in voxels and quarter turns', () => {
    expect(placed.bakedLayouts).toEqual([
      { gltfPath: '/layouts/wing.glb', position: [0, 0, 0], rotationY: 0, layoutName: 'wing', roomInstanceId: 'a' },
      { gltfPath: '/layouts/wing.glb', position: [12, 0, 0], rotationY: 1, layoutName: 'wing', roomInstanceId: 'b' },
    ]);
  });

  it("draws the rooms' own objects at their placements, as compileMap does", () => {
    expect(placed.instances).toEqual(api.rooms.compileMap(map, rooms).instances);
    expect(placed.instances.map((i) => i.position)).toEqual([[0, 0, 12]]);
  });
});
