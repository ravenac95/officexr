import { readFileSync, writeFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { ObjectInstance, Vec3 } from '@officexr/sdk';
import { TsSimCore } from './TsSimCore.ts';
import { recordTrace, type SimMap, type SimTrace, type SimTraceFrame } from './trace.ts';
import type { SimInput, SimKinds, SimStepResult } from './SimCore.ts';

/** Re-record the committed traces with `SIM_TRACES_UPDATE=1 pnpm --filter @officexr/world test sim`. */
const UPDATE = process.env.SIM_TRACES_UPDATE === '1';
const TRACE_DIR = new URL('./traces/', import.meta.url);

const VOXEL = 0.5;
const box = (id: string, size: Vec3) => ({ id, localAABB: { min: { x: 0, y: 0, z: 0 }, max: size } });
const KINDS: SimKinds = {
  voxelSize: VOXEL,
  kinds: [
    box('floor', { x: 40, y: 1, z: 40 }),
    box('small-floor', { x: 4, y: 1, z: 4 }),
    box('wall', { x: 1, y: 3, z: 40 }),
    box('ledge', { x: 4, y: 0.4, z: 40 }),
    {
      ...box('ramp', { x: 6, y: 1.5, z: 40 }),
      // A wedge rising along +x: its slope and its tall back face.
      colliderShape: {
        kind: 'trimesh',
        positions: [0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1],
        indices: [0, 1, 2, 0, 2, 3, 4, 2, 1, 4, 5, 2],
      },
    },
  ],
};

/** `kindId` placed with its min corner at world `min`. */
const place = (kindId: string, min: Vec3): ObjectInstance => ({
  id: `${kindId}@${min.x},${min.z}`,
  sourceCommandId: 'test',
  kindId,
  position: [min.x / VOXEL, min.y / VOXEL, min.z / VOXEL],
});

const mapOf = (...instances: ObjectInstance[]): SimMap => ({
  worldObjects: { cubeSize: VOXEL, instances },
  kinds: KINDS,
  bakedColliders: [],
});

/** A baked room's layout instance: collides through the baked colliders, never as an AABB. */
const layoutWall: ObjectInstance = { ...place('wall', { x: 100, y: 0, z: -20 }), id: 'room/layout/wall' };

const FLOOR = place('floor', { x: -20, y: -1, z: -20 });
/** Body root of a character standing on the floor's top (y = 0): BODY_Y − charRadius below. */
const ON_FLOOR = { x: 0, y: -0.5, z: 0 };
const SPAWN = { x: 0, y: 0, z: 0 };

const BASE: SimInput = {
  dtSec: 1 / 60,
  nowMs: 0,
  dir: { x: 0, z: 0 },
  speed: 3,
  running: false,
  yaw: 0,
  jump: false,
  settings: {
    charRadius: 0.4,
    movementBlockThreshold: 0.9,
    jumpVelocity: 8,
    airControl: 0.2,
    maxJumps: 2,
    landingEaseMs: 120,
  },
  spawnPoints: [SPAWN],
};

/** `count` frames continuing the clock from `start`, each with `step` merged over the base input. */
function frames(start: number, count: number, step: Partial<SimInput> = {}): SimTraceFrame[] {
  return Array.from({ length: count }, (_, i) => ({
    step: { ...BASE, nowMs: ((start + i) * 1000) / 60, ...step },
  }));
}

const EAST = { dir: { x: 1, z: 0 }, yaw: -Math.PI / 2 };

/** Teleport onto `at`, settle for 10 frames, then the scenario's frames. */
function scenario(at: Vec3, ...parts: Array<(start: number) => SimTraceFrame[]>): SimTraceFrame[] {
  const out = frames(0, 10);
  out[0] = { ...out[0], teleport: at };
  for (const part of parts) {
    out.push(...part(out.length));
  }
  return out;
}

interface Scenario {
  map: SimMap;
  inputs: SimTraceFrame[];
  check(outputs: SimStepResult[]): void;
}

const last = (outputs: SimStepResult[]) => outputs[outputs.length - 1];

const SCENARIOS: Record<string, Scenario> = {
  'walk-into-wall': {
    map: mapOf(FLOOR, place('wall', { x: 3, y: 0, z: -20 })),
    inputs: scenario(ON_FLOOR, (s) => frames(s, 90, EAST)),
    check(outputs) {
      const end = last(outputs);
      expect(end.newPos.x).toBeLessThan(3 - 0.4 + 0.01);
      expect(end.newPos.x).toBeGreaterThan(2.5);
      expect(end.moved).toBe(false);
      expect(end.animState).toBe('idle');
    },
  },
  'step-up-ledge': {
    map: mapOf(FLOOR, place('ledge', { x: 3, y: 0, z: -20 })),
    inputs: scenario(ON_FLOOR, (s) => frames(s, 90, EAST)),
    check(outputs) {
      const end = last(outputs);
      expect(end.newPos.x).toBeGreaterThan(4);
      expect(end.newPos.y).toBeCloseTo(-0.1, 1);
    },
  },
  slope: {
    map: mapOf(FLOOR, place('ramp', { x: 2, y: 0, z: -20 })),
    inputs: scenario(ON_FLOOR, (s) => frames(s, 90, EAST)),
    check(outputs) {
      const end = last(outputs);
      expect(end.newPos.x).toBeGreaterThan(4);
      expect(end.newPos.y).toBeGreaterThan(ON_FLOOR.y + 0.3);
      expect(end.isGrounded).toBe(true);
    },
  },
  jump: {
    map: mapOf(FLOOR),
    inputs: scenario(ON_FLOOR, (s) => frames(s, 1, { jump: true }), (s) => frames(s, 60)),
    check(outputs) {
      const peak = Math.max(...outputs.map((o) => o.newPos.y));
      expect(peak).toBeGreaterThan(ON_FLOOR.y + 0.5);
      expect(outputs[11].isAirborne).toBe(true);
      const end = last(outputs);
      expect(end.isAirborne).toBe(false);
      // The controller reports grounded within its offset, which zeroes the fall before full contact.
      expect(end.newPos.y - ON_FLOOR.y).toBeGreaterThanOrEqual(0);
      expect(end.newPos.y - ON_FLOOR.y).toBeLessThan(0.05);
    },
  },
  'fall-respawn': {
    map: mapOf(place('small-floor', { x: -2, y: -1, z: -2 })),
    inputs: scenario(ON_FLOOR, (s) => frames(s, 120, EAST)),
    check(outputs) {
      const respawn = outputs.findIndex((o) => o.respawned);
      expect(respawn).toBeGreaterThan(10);
      expect(outputs[respawn].newPos).toEqual({ x: 0, y: 4, z: 0 });
      expect(outputs[respawn].velY).toBe(0);
    },
  },
  'baked-wall': {
    // The wall instance sits far away, so a stop at x = 3 can only come from the baked collider.
    map: {
      ...mapOf(FLOOR, layoutWall),
      bakedColliders: [
        {
          roomInstanceId: 'room',
          position: { x: 2, y: 0, z: 0 },
          yawRad: 0,
          colliders: [
            { type: 'cuboid', center: { x: 0.5, y: 1.5, z: 0 }, halfExtents: { x: 0.5, y: 1.5, z: 20 } },
          ],
        },
      ],
    },
    inputs: scenario(ON_FLOOR, (s) => frames(s, 90, EAST)),
    check(outputs) {
      const end = last(outputs);
      expect(end.newPos.x).toBeLessThan(2.5 - 0.4 + 0.01);
      expect(end.newPos.x).toBeGreaterThan(1.5);
      expect(end.moved).toBe(false);
    },
  },
  'baked-pending': {
    // The layout's GLB hasn't loaded: its instances are neither colliders nor AABBs.
    map: {
      ...mapOf(FLOOR, { ...place('wall', { x: 3, y: 0, z: -20 }), id: 'room/layout/wall' }),
      bakedColliders: [{ roomInstanceId: 'room', position: { x: 0, y: 0, z: 0 }, yawRad: 0, colliders: [] }],
    },
    inputs: scenario(ON_FLOOR, (s) => frames(s, 90, EAST)),
    check(outputs) {
      expect(last(outputs).newPos.x).toBeGreaterThan(4);
    },
  },
  'peer-bump': {
    map: mapOf(FLOOR),
    inputs: scenario(ON_FLOOR, (s) =>
      frames(s, 60, EAST).map((f) => ({ ...f, peers: [{ id: 'peer', pos: { x: 2, y: -0.5, z: 0 } }] })),
    ),
    check(outputs) {
      const bumps = outputs.flatMap((o) => o.bumps);
      expect(bumps).toHaveLength(1);
      expect(bumps[0].otherId).toBe('peer');
      expect(bumps[0].normal.x).toBeLessThan(0);
      expect(last(outputs).newPos.x).toBeLessThan(2 - 0.8 + 0.01);
    },
  },
};

/** One frame per line, so a re-record diffs per frame. */
function formatTrace(trace: SimTrace): string {
  const lines = (rows: unknown[]) => rows.map((r) => JSON.stringify(r)).join(',\n');
  return (
    `{"kind":${JSON.stringify(trace.kind)},\n"map":${JSON.stringify(trace.map)},\n` +
    `"inputs":[\n${lines(trace.inputs)}\n],\n"outputs":[\n${lines(trace.outputs)}\n]}\n`
  );
}

function expectClose(actual: unknown, golden: unknown, path = 'outputs'): void {
  if (typeof golden === 'number') {
    expect(typeof actual, path).toBe('number');
    expect(Math.abs((actual as number) - golden), path).toBeLessThan(1e-6);
  } else if (golden !== null && typeof golden === 'object') {
    expect(Object.keys(actual as object).sort(), path).toEqual(Object.keys(golden).sort());
    for (const [k, v] of Object.entries(golden)) {
      expectClose((actual as Record<string, unknown>)[k], v, `${path}.${k}`);
    }
  } else {
    expect(actual, path).toEqual(golden);
  }
}

describe('TsSimCore movement traces', () => {
  for (const [name, { map, inputs, check }] of Object.entries(SCENARIOS)) {
    it(name, () => {
      const sim = new TsSimCore(BASE.settings.charRadius);
      const trace = recordTrace(sim, map, inputs);
      sim.dispose();

      const file = new URL(`${name}.json`, TRACE_DIR);
      if (UPDATE) {
        writeFileSync(file, formatTrace(trace));
      }
      const golden = JSON.parse(readFileSync(file, 'utf8')) as SimTrace;
      expect(JSON.parse(JSON.stringify({ map: trace.map, inputs: trace.inputs }))).toEqual({
        map: golden.map,
        inputs: golden.inputs,
      });
      expectClose(trace.outputs, golden.outputs);
      check(trace.outputs);
    });
  }
});

describe('TsSimCore stair climb', () => {
  const STAIRS_KINDS: SimKinds = {
    voxelSize: VOXEL,
    kinds: [
      box('floor', { x: 40, y: 1, z: 40 }),
      box('landing', { x: 4, y: 4, z: 4 }),
      box('wall', { x: 12, y: 6, z: 0.5 }),
      {
        ...box('stairs', { x: 4, y: 4, z: 4 }),
        colliderShape: { kind: 'compound-steps', stepCount: 8, stepRise: 0.5, stepRun: 0.5, stepDepth: 4 },
      },
    ],
  };

  /** 8 risers of 0.5 m ascending toward -X onto a 4 m high landing, with a wall just past the walking line. */
  function stairs(): TsSimCore {
    const sim = new TsSimCore(BASE.settings.charRadius);
    sim.setStaticColliders(
      {
        cubeSize: VOXEL,
        instances: [
          place('floor', { x: -5, y: -1, z: -5 }),
          place('landing', { x: 0, y: 0, z: 0 }),
          place('stairs', { x: 4, y: 0, z: 0 }),
          place('wall', { x: 0, y: 0, z: 2.5 }),
        ],
      },
      STAIRS_KINDS,
      [],
    );
    sim.teleport({ x: 10, y: -0.5, z: 2 });
    return sim;
  }

  const walk = (sim: TsSimCore, speed: number, dir: { x: number; z: number }, jump = false) =>
    sim.step({ ...BASE, dir, speed, jump }).newPos;

  /** Walks -X until the first hop, then `extra` more frames; returns the hop frame's pose. */
  function walkToFirstHop(sim: TsSimCore, speed: number) {
    let prev = sim.position();
    for (let i = 0; i < 4000; i++) {
      const pos = walk(sim, speed, { x: -1, z: 0 });
      if (pos.y - prev.y > 0.3) {
        return pos;
      }
      prev = pos;
    }
    throw new Error('never hopped');
  }

  for (const speed of [0.5, 1, 3]) {
    it(`climbs 8 risers in 8 hops without sagging back at ${speed} m/s`, () => {
      const sim = stairs();
      let prev = sim.position();
      let maxY = prev.y;
      let hops = 0;
      let maxStepX = 0;
      for (let i = 0; i < 4000 && prev.x > 1; i++) {
        const pos = walk(sim, speed, { x: -1, z: 0 });
        if (pos.y - prev.y > 0.3) {
          hops++;
          maxStepX = Math.max(maxStepX, Math.abs(pos.x - prev.x));
        }
        expect(pos.y).toBeGreaterThan(maxY - 0.02);
        maxY = Math.max(maxY, pos.y);
        prev = pos;
      }
      sim.dispose();
      expect(hops).toBe(8);
      expect(maxStepX).toBeLessThanOrEqual(speed / 60 + 0.01);
      expect(prev.y).toBeCloseTo(3.5, 1);
    });
  }

  it('lets a jump during the hold clear more than 0.5 m', () => {
    const sim = stairs();
    const hop = walkToFirstHop(sim, 1);
    let maxY = hop.y;
    maxY = Math.max(maxY, walk(sim, 1, { x: -1, z: 0 }, true).y);
    for (let i = 0; i < 20; i++) {
      maxY = Math.max(maxY, walk(sim, 1, { x: -1, z: 0 }).y);
    }
    sim.dispose();
    expect(maxY - hop.y).toBeGreaterThan(0.5);
  });

  it('drops the ball soon after reversing off the tread mid-hold', () => {
    const sim = stairs();
    const hop = walkToFirstHop(sim, 1);
    let y = hop.y;
    for (let i = 0; i < 15; i++) {
      y = walk(sim, 1, { x: 1, z: 0 }).y;
    }
    sim.dispose();
    expect(y).toBeLessThan(hop.y - 0.3);
  });

  it('ends the hold when blocked sideways mid-hold', () => {
    const sim = stairs();
    const hop = walkToFirstHop(sim, 1);
    let y = hop.y;
    for (let i = 0; i < 30; i++) {
      y = walk(sim, 1, { x: 0, z: 1 }).y;
    }
    sim.dispose();
    expect(y).toBeLessThan(hop.y - 0.1);
  });
});
