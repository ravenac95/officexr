// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { createActions, createStore, type PlacedProp, type PropOp } from '@officexr/sdk';
import { createPlaceMode, PLACE_AHEAD_M, type PlacementHost } from '../motion/place-mode.ts';
import { PlacementFrame, placementPreview } from './placementFrame.ts';

/** One voxel is 1 m and every footprint 1 m square and tall. */
const geometry = {
  voxelSize: 1,
  worldAABB: (position: readonly number[]) => ({
    min: [position[0]!, position[1]!, position[2]!] as const,
    max: [position[0]! + 1, position[1]! + 1, position[2]! + 1] as const,
  }),
  worldAABBOfInstance(i: { position: readonly number[] }) {
    return this.worldAABB(i.position);
  },
};

function setup() {
  const store = createStore({ selfId: 'me', officeId: 'o' });
  const actions = createActions(store);
  actions.upsertPlayer({ id: 'me', pos: { x: 0, y: 0, z: 0 }, yaw: 0 });
  const sent: PropOp[] = [];
  const mode = createPlaceMode();
  const host: PlacementHost = {
    mode,
    send: async (op) => {
      sent.push(op);
      return { ok: true };
    },
  };
  const target = new EventTarget();
  const locked = { value: false };
  const frame = new PlacementFrame({ host, getState: () => store.getState(), geometry, target, pointerLocked: () => locked.value });
  const key = (key: string) => target.dispatchEvent(new KeyboardEvent('keydown', { key, cancelable: true }));
  const mouseDown = () => target.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
  return { store, mode, sent, frame, key, mouseDown, locked };
}

describe('PlacementFrame', () => {
  it('keeps the ghost ahead of the player only while build mode is open', () => {
    const { mode, frame } = setup();
    frame.step();
    expect(mode.get().ghost).toBeNull();

    mode.setOpen(true);
    frame.step();
    const ghost = mode.get().ghost;
    // Yaw 0 faces -Z; the 1 m footprint centres on the point ahead.
    expect(ghost).toMatchObject({ valid: true, targetId: null });
    expect(ghost!.position[2]).toBeCloseTo(-PLACE_AHEAD_M - 0.5);
    frame.dispose();
  });

  it('marks a ghost that overlaps a placed prop invalid and points at that prop', () => {
    const { mode, frame, store } = setup();
    mode.setOpen(true);
    frame.step();
    const spot = mode.get().ghost!.position;
    store.setState(() => ({ placedProps: [{ id: 'p1', kindId: mode.get().kindId, position: spot, yawDeg: 0 }] as PlacedProp[] }));
    frame.step();
    expect(mode.get().ghost).toMatchObject({ valid: false, targetId: 'p1' });
    frame.dispose();
  });

  it('hears its keys only while open: Enter and the HUD Place button place, R turns the ghost', async () => {
    const { mode, frame, key, sent } = setup();
    key('Enter');
    expect(sent).toEqual([]);

    mode.setOpen(true);
    frame.step();
    key('r');
    expect(mode.get().yawDeg).toBe(90);
    frame.step();
    key('Enter');
    mode.place();
    expect(sent).toEqual([
      expect.objectContaining({ op: 'place', yawDeg: 90 }),
      expect.objectContaining({ op: 'place', yawDeg: 90 }),
    ]);

    mode.setOpen(false);
    key('Enter');
    mode.place();
    expect(sent).toHaveLength(2);
    frame.dispose();
  });

  it('places on a left press only while the pointer is locked', () => {
    const { mode, frame, mouseDown, locked, sent } = setup();
    mode.setOpen(true);
    frame.step();
    mouseDown();
    expect(sent).toHaveLength(0);
    locked.value = true;
    mouseDown();
    expect(sent).toHaveLength(1);
    frame.dispose();
  });
});

describe('placementPreview', () => {
  it("is the ghost with the mode's kind and turn, and the box of the prop it points at", () => {
    const mode = createPlaceMode();
    const placed = [{ id: 'p1', kindId: 'prop_pool', position: [2, 0, 3], yawDeg: 0 }] as PlacedProp[];
    expect(placementPreview(mode.get(), placed, geometry)).toEqual({ ghost: null, target: null });

    mode.setOpen(true);
    mode.setYaw(180);
    mode.setGhost({ position: [0, 0, -4], valid: true, targetId: 'p1' });
    expect(placementPreview(mode.get(), placed, geometry)).toEqual({
      ghost: { kindId: mode.get().kindId, position: [0, 0, -4], yawDeg: 180, valid: true },
      target: { min: [2, 0, 3], max: [3, 1, 4] },
    });
  });
});
