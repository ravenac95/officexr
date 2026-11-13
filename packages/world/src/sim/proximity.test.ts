import { readFileSync, writeFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { PlayerState } from '@officexr/sdk';
import { TsSimCore } from './TsSimCore.ts';
import { EXTRAPOLATION_CAP_S, extrapolatePos } from './extrapolate.ts';
import { recordProximityTrace, type ProximityTrace, type ProximityTraceFrame } from './trace.ts';

/** Re-record the committed traces with `SIM_TRACES_UPDATE=1 pnpm --filter @officexr/world test sim`. */
const UPDATE = process.env.SIM_TRACES_UPDATE === '1';
const TRACE_DIR = new URL('./traces/', import.meta.url);

const SETTINGS = { charRadius: 0.3, proximityRadius: 1.5, proximityOuterRadius: 3, proximityEnterDebounceMs: 300 };
const FRAME_MS = 16;

type Layout = Record<string, number>;

const player = (id: string, x: number): PlayerState =>
  ({ id, pos: { x, y: 0, z: 0 }, vel: { x: 0, y: 0, z: 0 } }) as PlayerState;

/** One tick per layout, each mapping a player id to its x; alice is the local player. */
function ticks(layouts: Layout[], start = 0): ProximityTraceFrame[] {
  return layouts.map((layout, i) => ({
    players: Object.fromEntries(Object.entries(layout).map(([id, x]) => [id, player(id, x)])),
    input: {
      voiceZones: [],
      selfId: 'alice',
      settings: SETTINGS,
      nowMs: (start + i) * FRAME_MS,
      dtSec: FRAME_MS / 1000,
    },
  }));
}

const hold = (layout: Layout, count: number): Layout[] => Array.from({ length: count }, () => layout);

/** Straight-line walk of `id` from `from` to `to` over `count` ticks, others fixed. */
const walk = (others: Layout, id: string, from: number, to: number, count: number): Layout[] =>
  Array.from({ length: count }, (_, i) => ({ ...others, [id]: from + ((to - from) * i) / (count - 1) }));

const kinds = (t: ProximityTrace) => t.outputs.flatMap((o) => o.events.map((e) => `${e.kind.slice(10)} ${e.otherId}`));

const SCENARIOS: Record<string, { layouts: Layout[]; check(t: ProximityTrace): void }> = {
  'proximity-pair': {
    layouts: [
      ...walk({ alice: 0 }, 'bob', 10, 1.5, 60),
      ...hold({ alice: 0, bob: 1.5 }, 40),
      ...walk({ alice: 0 }, 'bob', 1.5, 10, 60),
    ],
    check(t) {
      expect(kinds(t)).toEqual(['entering bob', 'entered bob', 'exiting bob', 'exited bob']);
      expect(t.outputs.some((o) => o.areas.length === 1)).toBe(true);
      expect(t.outputs[t.outputs.length - 1].areas).toEqual([]);
    },
  },
  'proximity-debounce': {
    layouts: [
      // Inside the inner ring for 160 ms, under the 300 ms debounce, then back out to the outer band.
      ...hold({ alice: 0, bob: 5 }, 3),
      ...hold({ alice: 0, bob: 2 }, 10),
      ...hold({ alice: 0, bob: 3 }, 10),
      // Held long enough this time.
      ...hold({ alice: 0, bob: 2 }, 30),
    ],
    check(t) {
      expect(kinds(t)).toEqual(['entering bob', 'entered bob']);
      const entered = t.outputs.findIndex((o) => o.events.some((e) => e.kind === 'proximity:entered'));
      expect(entered).toBeGreaterThanOrEqual(23 + Math.ceil(300 / FRAME_MS));
    },
  },
  'proximity-three-person-area': {
    layouts: [
      ...hold({ alice: 0, bob: 1.5 }, 40),
      ...walk({ alice: 0, bob: 1.5 }, 'carol', 5, -1.5, 30),
      ...hold({ alice: 0, bob: 1.5, carol: -1.5 }, 30),
      // Bob walks out of the outer ring; alice and carol stay in the area.
      ...walk({ alice: 0, carol: -1.5 }, 'bob', 1.5, 12, 60),
      ...hold({ alice: 0, bob: 12, carol: -1.5 }, 20),
    ],
    check(t) {
      const sizes = t.outputs.map((o) => o.areas[0]?.members.length ?? 0);
      expect(Math.max(...sizes)).toBe(3);
      expect(sizes[sizes.length - 1]).toBe(2);
      const radii = t.outputs.flatMap((o) => (o.areas[0] ? [o.areas[0].radius] : []));
      radii.forEach((r, i) => {
        expect(r).toBeGreaterThanOrEqual(i === 0 ? 0 : radii[i - 1] - 1e-9);
      });
      expect(kinds(t)).toContain('exited bob');
    },
  },
};

function formatTrace(trace: ProximityTrace): string {
  const lines = (rows: unknown[]) => rows.map((r) => JSON.stringify(r)).join(',\n');
  return `{"kind":"proximity",\n"inputs":[\n${lines(trace.inputs)}\n],\n"outputs":[\n${lines(trace.outputs)}\n]}\n`;
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

describe('TsSimCore proximity traces', () => {
  for (const [name, { layouts, check }] of Object.entries(SCENARIOS)) {
    it(name, () => {
      const sim = new TsSimCore(SETTINGS.charRadius);
      const trace = recordProximityTrace(sim, ticks(layouts));
      sim.dispose();

      const file = new URL(`${name}.json`, TRACE_DIR);
      if (UPDATE) {
        writeFileSync(file, formatTrace(trace));
      }
      const golden = JSON.parse(readFileSync(file, 'utf8')) as ProximityTrace;
      expect(JSON.parse(JSON.stringify(trace.inputs))).toEqual(golden.inputs);
      expectClose(JSON.parse(JSON.stringify(trace.outputs)), golden.outputs);
      check(trace);
    });
  }
});

describe('extrapolatePos', () => {
  const peer = { pos: { x: 1, y: 0, z: 2 }, vel: { x: 2, y: 0, z: -1 }, tRecv: 1000 };

  it('advances by velocity since the last update', () => {
    expect(extrapolatePos(peer, 1050)).toEqual({ x: 1.1, y: 0, z: 1.95 });
  });

  it('freezes once the update is older than the cap', () => {
    const capped = extrapolatePos(peer, 1000 + EXTRAPOLATION_CAP_S * 1000);
    expect(extrapolatePos(peer, 5000)).toEqual(capped);
    expect(capped.x).toBeCloseTo(1.2);
  });

  it('leaves a local player, which has no receive time, where it is', () => {
    expect(extrapolatePos({ ...peer, tRecv: undefined }, 5000)).toEqual(peer.pos);
  });
});
