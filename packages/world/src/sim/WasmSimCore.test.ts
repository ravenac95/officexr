import { readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { describe, expect, it } from 'vitest';
import * as wasm from '@officexr/sim-wasm';
import type { Vec3 } from '@officexr/sdk';
import { TsSimCore } from './TsSimCore.ts';
import { WasmSimCore } from './WasmSimCore.ts';
import { recordProximityTrace, recordTrace, type ProximityFrameOutput, type ProximityTrace, type SimTrace } from './trace.ts';
import type { SimCore, SimStepResult } from './SimCore.ts';

/** The Rust replay's tolerance (`crates/officexr-sim/tests/replay_traces.rs`). */
const TOLERANCE = 1e-4;
const TRACE_DIR = new URL('./traces/', import.meta.url);

wasm.initSync({ module: readFileSync(createRequire(import.meta.url).resolve('@officexr/sim-wasm/officexr_sim_wasm_bg.wasm')) });

const IMPLEMENTATIONS: Record<string, (charRadius: number) => SimCore> = {
  ts: (r) => new TsSimCore(r),
  wasm: (r) => new WasmSimCore(new wasm.WasmSim(r)),
};

const maxDiff = (a: Vec3, b: Vec3) => Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y), Math.abs(a.z - b.z));

function expectSteps(actual: SimStepResult[], golden: SimStepResult[]): void {
  expect(actual).toHaveLength(golden.length);
  actual.forEach((a, i) => {
    const g = golden[i];
    const exact = (r: SimStepResult) => ({
      moved: r.moved,
      animState: r.animState,
      isGrounded: r.isGrounded,
      hasFloorUnderneath: r.hasFloorUnderneath,
      respawned: r.respawned,
      isAirborne: r.isAirborne,
      broadcastYaw: r.broadcastYaw,
      bumps: r.bumps.map((b) => b.otherId),
    });
    expect(exact(a), `frame ${i}`).toEqual(exact(g));
    a.bumps.forEach((b, j) => {
      const n = Math.max(Math.abs(b.normal.x - g.bumps[j].normal.x), Math.abs(b.normal.z - g.bumps[j].normal.z));
      expect(n, `frame ${i} bump normal`).toBeLessThanOrEqual(TOLERANCE);
    });
    expect(maxDiff(a.newPos, g.newPos), `frame ${i} newPos`).toBeLessThanOrEqual(TOLERANCE);
    const vel = Math.max(maxDiff(a.broadcastVel, g.broadcastVel), Math.abs(a.velY - g.velY));
    expect(vel, `frame ${i} velocity`).toBeLessThanOrEqual(TOLERANCE);
  });
}

function expectProximity(actual: ProximityFrameOutput[], golden: ProximityFrameOutput[]): void {
  expect(actual).toHaveLength(golden.length);
  actual.forEach((a, i) => {
    const g = golden[i];
    const exact = (o: ProximityFrameOutput) => ({
      events: o.events,
      pairStates: o.pairStates,
      playerStates: o.playerStates,
      areas: o.areas.map((x) => [x.id, x.members]),
      focus: o.conversationFocus === null,
    });
    expect(exact(a), `frame ${i}`).toEqual(exact(g));
    let worst = 0;
    a.areas.forEach((x, j) => {
      const y = g.areas[j];
      worst = Math.max(worst, Math.abs(x.center.x - y.center.x), Math.abs(x.center.z - y.center.z), Math.abs(x.radius - y.radius));
    });
    if (a.conversationFocus && g.conversationFocus) {
      worst = Math.max(worst, Math.abs(a.conversationFocus.x - g.conversationFocus.x), Math.abs(a.conversationFocus.z - g.conversationFocus.z));
    }
    expect(worst, `frame ${i} geometry`).toBeLessThanOrEqual(TOLERANCE);
  });
}

const traces = readdirSync(TRACE_DIR)
  .filter((f) => f.endsWith('.json'))
  .sort()
  .map((f) => [f.replace(/\.json$/, ''), JSON.parse(readFileSync(new URL(f, TRACE_DIR), 'utf8'))] as const);

describe.each(Object.entries(IMPLEMENTATIONS))('%s SimCore replays the committed traces', (_impl, create) => {
  it.each(traces)('%s', (_name, trace: SimTrace | ProximityTrace) => {
    if (trace.kind === 'proximity') {
      const sim = create(trace.inputs[0].input.settings.charRadius);
      expectProximity(recordProximityTrace(sim, trace.inputs).outputs, trace.outputs);
      sim.dispose();
      return;
    }
    const sim = create(trace.inputs[0].step.settings.charRadius);
    expectSteps(recordTrace(sim, trace.map, trace.inputs).outputs, trace.outputs);
    sim.dispose();
  });
});

describe.each(Object.entries(IMPLEMENTATIONS))('%s SimCore standingAt', (_impl, create) => {
  const R = 0.4;
  const VOXEL = 1;
  const box = (id: string, size: Vec3) => ({ id, localAABB: { min: { x: 0, y: 0, z: 0 }, max: size } });
  const kinds = { voxelSize: VOXEL, kinds: [box('slab', { x: 20, y: 0.15, z: 20 }), box('crate', { x: 1, y: 1, z: 1 })] };
  const at = (kindId: string, position: [number, number, number]) => ({ id: kindId, sourceCommandId: 't', kindId, position });

  /** A floor slab whose top sits 0.15 m above y = 0, the way a placed floor tile does. */
  function simOn(...extra: ReturnType<typeof at>[]): SimCore {
    const sim = create(R);
    sim.setStaticColliders({ cubeSize: VOXEL, instances: [at('slab', [-10, 0, -10]), ...extra] }, kinds, []);
    return sim;
  }

  it('stands the probe on a floor slab that starts above the chair footprint', () => {
    const sim = simOn();
    expect(sim.standingAt({ x: 0, y: 0, z: 0 }, R)).toBe(true);
    sim.dispose();
  });

  it('still sees an obstacle standing on the slab', () => {
    const sim = simOn(at('crate', [0, 0.15, 0]));
    expect(sim.standingAt({ x: 0.5, y: 0, z: 0 }, R)).toBe(false);
    expect(sim.standingAt({ x: 3, y: 0, z: 0 }, R)).toBe(true);
    sim.dispose();
  });

  it("ignores the player's own body", () => {
    const sim = simOn();
    sim.teleport({ x: 0, y: 0.15, z: 0 });
    expect(sim.standingAt({ x: 0, y: 0, z: 0 }, R)).toBe(true);
    sim.dispose();
  });
});
