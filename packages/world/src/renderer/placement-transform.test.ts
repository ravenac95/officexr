import { describe, expect, it } from 'vitest';
import { createInstanceGeometry } from '../app/geometry-service.ts';
import { compileMap } from '../scenes/compile-map.ts';
import { emptyRoomDocument, newPlaceObject } from '../scenes/commands.ts';
import { emptyMapDocument } from '../scenes/map-document.ts';
import type { WorldObjectKind } from '../scenes/world-object-kinds-schema.ts';
import { VOXEL_SIZE } from './config.ts';
import { placementTransform } from './placement-transform.ts';

type Vec = readonly [number, number, number];

/** `v` turned `yaw` radians about +Y, in the sense of a Bevy or glTF `rotation.y`. */
const rotateY = ([x, y, z]: Vec, yaw: number): Vec => [
  x * Math.cos(yaw) + z * Math.sin(yaw),
  y,
  -x * Math.sin(yaw) + z * Math.cos(yaw),
];
const add = (a: Vec, b: Vec): Vec => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const near = (v: readonly number[]) => v.map((x) => expect.closeTo(x, 9));

describe('placementTransform', () => {
  const room = emptyRoomDocument('room');
  room.commands.push(newPlaceObject({ kindId: 'k', position: [2, 1, 3] }));

  it.each([0, 1, 2, 3] as const)('moves a cube onto its compileMap AABB at rotationY %i', (rotationY) => {
    const placement = { position: [7, 2, -5] as [number, number, number], rotationY };
    const map = emptyMapDocument('m');
    map.rooms = [{ id: 'p', roomName: 'room', ...placement }];
    const [inst] = compileMap(map, new Map([['room', room]]), VOXEL_SIZE).instances;

    const { position, rotation } = placementTransform(placement);
    const corners = [2, 3].flatMap((cx) =>
      [1, 2].flatMap((cy) => [3, 4].map((cz): Vec => [cx * VOXEL_SIZE, cy * VOXEL_SIZE, cz * VOXEL_SIZE])),
    );
    const world = corners.map((c) => add(position, rotateY(c, rotation[1])));
    const min = [0, 1, 2].map((i) => Math.min(...world.map((c) => c[i])) / VOXEL_SIZE);
    const max = [0, 1, 2].map((i) => Math.max(...world.map((c) => c[i])) / VOXEL_SIZE);

    const [x, y, z] = inst.position;
    expect(min).toEqual(near([x, y, z]));
    expect(max).toEqual(near([x + 1, y + 1, z + 1]));
  });

  describe('compiled yaw matches the rigid room transform', () => {
    // Non-square and off-centre mesh origin, so any pivot mistake shows.
    const kind = {
      id: 'k',
      localAABB: { min: { x: -0.2, y: 0, z: -0.1 }, max: { x: 1.3, y: 1, z: 0.5 } },
    } as WorldObjectKind;
    const geom = createInstanceGeometry({
      catalog: { getKind: (id) => (id === 'k' ? kind : undefined) },
      voxelSize: VOXEL_SIZE,
    });

    const cases = [0, 90, 45].flatMap((yawDeg) =>
      ([0, 1, 2, 3] as const).map((rotationY) => ({ yawDeg, rotationY })),
    );
    it.each(cases)('yaw $yawDeg, rotationY $rotationY', ({ yawDeg, rotationY }) => {
      const objectRoom = emptyRoomDocument('room');
      objectRoom.commands.push(newPlaceObject({ kindId: 'k', position: [3, 1, 5], yawDeg }));
      const placement = { position: [7, 2, -4] as [number, number, number], rotationY };
      const map = emptyMapDocument('m');
      map.rooms = [{ id: 'p', roomName: 'room', ...placement }];
      const [inst] = compileMap(
        map,
        new Map([['room', objectRoom]]),
        VOXEL_SIZE,
        undefined,
        undefined,
        geom.worldAABB,
      ).instances;

      const room = placementTransform(placement);
      const local = geom.meshTransform([3, 1, 5], 'k', yawDeg);
      const rigidPosition = add(room.position, rotateY(local.position, room.rotation[1]));
      const rigidYaw = room.rotation[1] + local.rotationY;
      const compiled = geom.meshTransform(inst.position, 'k', inst.yawDeg);
      expect(compiled.position).toEqual(near(rigidPosition));
      expect([Math.cos(compiled.rotationY), Math.sin(compiled.rotationY)]).toEqual(
        near([Math.cos(rigidYaw), Math.sin(rigidYaw)]),
      );
    });
  });

  it('is the identity without a placement', () => {
    expect(placementTransform()).toEqual({ position: [0, 0, 0], rotation: [0, 0, 0] });
  });
});
