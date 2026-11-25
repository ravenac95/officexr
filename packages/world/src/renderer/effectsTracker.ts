import { SPELLS, travelMs, type Bus, type OfficeState, type SpellCast, type Vec3 } from '@officexr/sdk';
import type { Effect } from '@officexr/render-wasm';
import { POKE_REACH_M } from '../motion/poke.ts';
import { feetPosition } from '../physics/rules.ts';

/** `PokeSpark.tsx`'s delay and life, and how long a bolt's burst outlasts its landing (the renderer's). */
export const POKE_SPARK_MS = 100 + 550;
export const BOLT_IMPACT_MS = 900;
/** `SpellBolts.tsx` keeps the last twelve. */
const MAX_BOLTS = 12;
/** A poke's spark sits out at this share of the reach, at hand height. */
const SPARK_REACH = 0.65;
const SPARK_HEIGHT_M = 1.05;

interface Spark {
  id: number;
  at: [number, number, number];
  startMs: number;
}

interface Bolt {
  id: number;
  cast: SpellCast;
  startMs: number;
}

const xyz = (v: Vec3): [number, number, number] => [v.x, v.y, v.z];

/**
 * The poke sparks and spell bolts playing, from the bus's `poke:received` and `spell:cast`, as
 * `setPlayers`' `effects` (`PokeSpark.tsx` and `SpellBolts.tsx`'s lists). Their sounds are
 * `SoundFrame`'s.
 */
export class EffectsTracker {
  private nextId = 0;
  private sparks: Spark[] = [];
  private bolts: Bolt[] = [];
  private readonly stops: (() => void)[];

  constructor(
    bus: Bus,
    private readonly store: { getState(): OfficeState },
    private readonly now: () => number = () => performance.now(),
  ) {
    this.stops = [
      bus.on('poke:received', ({ fromId }) => this.onPoke(fromId)),
      bus.on('spell:cast', ({ cast }) => {
        this.bolts = [...this.bolts.slice(-(MAX_BOLTS - 1)), { id: ++this.nextId, cast, startMs: this.now() }];
      }),
    ];
  }

  dispose(): void {
    for (const stop of this.stops) {
      stop();
    }
  }

  private onPoke(fromId: string) {
    const state = this.store.getState();
    const from = state.players[fromId];
    if (!from) {
      return;
    }
    const reach = POKE_REACH_M * SPARK_REACH;
    this.sparks.push({
      id: ++this.nextId,
      at: [
        from.pos.x - Math.sin(from.yaw) * reach,
        feetPosition(from.pos, state.worldSettings.charRadius).y + SPARK_HEIGHT_M,
        from.pos.z - Math.cos(from.yaw) * reach,
      ],
      startMs: this.now(),
    });
  }

  /** The effects at `nowMs`, dropping the finished ones. */
  effects(nowMs: number): Effect[] {
    this.sparks = this.sparks.filter((s) => nowMs - s.startMs < POKE_SPARK_MS);
    this.bolts = this.bolts.filter((b) => nowMs - b.startMs < travelMs(b.cast.spell, b.cast.distance) + BOLT_IMPACT_MS);
    const sparks = this.sparks.map((s): Effect => ({ kind: 'pokeSpark', id: s.id, at: s.at, age: (nowMs - s.startMs) / 1000 }));
    const bolts = this.bolts.map(({ id, cast, startMs }): Effect => {
      const spell = SPELLS[cast.spell];
      return {
        kind: 'bolt',
        id,
        origin: xyz(cast.origin),
        dir: xyz(cast.dir),
        distance: cast.distance,
        speed: spell.projectile.speedMps,
        color: spell.projectile.color,
        fire: spell.impact === 'fire',
        age: (nowMs - startMs) / 1000,
      };
    });
    return [...sparks, ...bolts];
  }
}
