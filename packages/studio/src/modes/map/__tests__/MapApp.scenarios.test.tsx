import { beforeEach, describe, expect, it } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { PickTarget } from '@officexr/render-wasm';
import { InMemoryMapStorage, emptyMapDocument } from '@officexr/world/scenes';
import { createTestApi } from '../../../test-harness/createTestApi.ts';
import { resolveMapPick } from '../mapPointerActions.ts';
import type { MapTool } from '../mapTools.ts';
import { dragRoomTo } from '../roomBoundarySnap.ts';
import { useMapDocument } from '../useMapDocument.ts';

/**
 * The Map editor's press routing against the real document store, with picks as data: what the
 * Bevy view reports for a press on a room's baked layout, one of its objects, a spawn marker, a
 * desk area or bare ground. Catches the c606f10 class (spawn and select routing on a room,
 * whichever of its meshes was hit) without a renderer.
 */

type Vec3 = [number, number, number];
const VOXEL = createTestApi().geometry.voxelSize;

beforeEach(() => globalThis.localStorage?.clear());

async function mapEditor() {
  const doc = { ...emptyMapDocument('default', 'Default'), deskAreas: [{ id: 'd1', min: { x: 0, z: 0 }, max: { x: 2, z: 2 } }] };
  const storage = new InMemoryMapStorage({ seed: [{ name: 'default', doc }] });
  const { result } = renderHook(() => useMapDocument({ storage }));
  await waitFor(() => expect(result.current.doc.name).toBe('default'));
  let room = '';
  act(() => {
    room = result.current.addRoom('snap-room', [4, 0, 0]);
    result.current.setSelection(null);
  });
  /** A left press as the canvas applies it. */
  const press = (target: PickTarget, point: Vec3 | null, tool: MapTool) => {
    const action = resolveMapPick(target, point, tool);
    act(() => {
      const map = result.current;
      if (action?.type === 'select') map.setSelection(action.selection);
      if (action?.type === 'deselect') map.setSelection(null);
      if (action?.type === 'place-spawn') map.addSpawn(action.at);
      if (action?.type === 'drag-room') map.setSelection({ kind: 'room', id: action.id });
    });
    return action;
  };
  return { map: () => result.current, room, press };
}

describe('Map editor scenarios', () => {
  it('Spawn tool: a press on a room drops a spawn at the exact hit, whichever mesh was hit', async () => {
    const { map, room, press } = await mapEditor();
    press({ type: 'layout', name: 'wing', roomInstanceId: room }, [1.5, 2.25, -3], 'spawn');
    press({ type: 'object', id: `${room}/c1` }, [2, 1, 0], 'spawn');
    expect(map().doc.spawnPoints.map((s) => s.position)).toEqual([
      [1.5, 2.25, -3],
      [2, 1, 0],
    ]);
  });

  it('Spawn tool: bare ground places nothing and keeps the selection', async () => {
    const { map, press } = await mapEditor();
    press({ type: 'ground' }, [1, 0, 1], 'spawn');
    expect(map().doc.spawnPoints).toEqual([]);
  });

  it('Select tool: a press on a room selects it, and bare ground clears the selection', async () => {
    const { map, room, press } = await mapEditor();
    press({ type: 'object', id: `${room}/c1` }, [0, 0, 0], 'select');
    expect(map().selection).toEqual({ kind: 'room', id: room });
    expect(map().doc.spawnPoints).toEqual([]);
    press({ type: 'ground' }, [9, 0, 9], 'select');
    expect(map().selection).toBeNull();
  });

  it('selects spawn markers and, in the Select tool, desk areas', async () => {
    const { map, press } = await mapEditor();
    press({ type: 'overlay', tag: 'desk:d1' }, [1, 0, 1], 'select');
    expect(map().selection).toEqual({ kind: 'desk-area', id: 'd1' });
    let spawn = '';
    act(() => {
      spawn = map().addSpawn([0, 1, 0]);
    });
    press({ type: 'overlay', tag: `spawn:${spawn}` }, null, 'move');
    expect(map().selection).toEqual({ kind: 'spawn', id: spawn });
  });

  it('Move tool: a press selects the room and its drag moves it on the voxel grid', async () => {
    const { map, room, press } = await mapEditor();
    const action = press({ type: 'object', id: `${room}/c1` }, [4 * VOXEL + 0.2, 1, 0.1], 'move');
    expect(map().selection).toEqual({ kind: 'room', id: room });
    expect(action).toEqual({ type: 'drag-room', id: room, at: [4 * VOXEL + 0.2, 1, 0.1] });
    const offset = { x: 0.2, z: 0.1 };
    const next = dragRoomTo({ x: 10 * VOXEL + 0.2, z: 3 * VOXEL + 0.1 }, offset, [4, 0, 0], null, [], VOXEL, 2);
    act(() => map().setRoomPosition(room, next!));
    expect(map().doc.rooms[0].position).toEqual([10, 0, 3]);
  });

  it('Desk area tool: a press on a room is left to the drawing gesture', async () => {
    const { map, room, press } = await mapEditor();
    expect(press({ type: 'object', id: `${room}/c1` }, [0, 0, 0], 'desk-area')).toBeNull();
    expect(press({ type: 'ground' }, [0, 0, 0], 'desk-area')).toBeNull();
    expect(map().selection).toBeNull();
  });
});
