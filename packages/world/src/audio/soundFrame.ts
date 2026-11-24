import type { Bus, OfficeState, SpellCast } from '@officexr/sdk';
import { boltAt, landingPoint } from '../motion/bolt.ts';
import { boltLoopCue, castCues, impactCues, pokeCue, SoundDirector, type SoundCue } from './cues.ts';
import type { SfxEngine } from './engine.ts';

export type SfxSink = Pick<SfxEngine, 'play' | 'tick' | 'setListener'>;

/** As many bolts as upstream's renderer kept in flight; an older one goes silent. */
const MAX_BOLTS = 12;

interface Bolt {
  id: number;
  cast: SpellCast;
  castAt: number;
}

/**
 * Feeds the sound engine each frame without a renderer: the listener is the
 * self player; every player's pose, hand and pokes become cues; and each spell
 * cast plays its cast, its flight's loop and its impact on landing.
 */
export class SoundFrame {
  private readonly director = new SoundDirector();
  private readonly bolts: Bolt[] = [];
  private nextBolt = 0;
  private readonly stops: (() => void)[];

  constructor(
    private readonly getState: () => OfficeState,
    bus: Bus,
    private readonly sfx: SfxSink,
  ) {
    this.stops = [
      bus.on('poke:received', ({ fromId }) => {
        const from = this.getState().players[fromId];
        if (from) {
          sfx.play(pokeCue(from.pos));
        }
      }),
      bus.on('spell:cast', ({ cast, at }) => {
        const bolt = { id: ++this.nextBolt, cast, castAt: at };
        this.bolts.push(bolt);
        const dropped = this.bolts.length > MAX_BOLTS ? this.bolts.shift() : undefined;
        if (dropped) {
          this.land(dropped, false);
        }
        castCues(cast).forEach((cue) => sfx.play(cue));
      }),
    ];
  }

  step(nowMs: number): void {
    const { sfx } = this;
    sfx.tick();
    const state = this.getState();
    const self = state.players[state.selfId];
    if (self) {
      sfx.setListener(self.pos);
    }
    for (const cue of this.director.frame(state.players, nowMs)) {
      sfx.play(cue);
    }
    for (const bolt of [...this.bolts]) {
      const flight = boltAt(bolt.cast, nowMs - bolt.castAt);
      if (flight.landed) {
        this.bolts.splice(this.bolts.indexOf(bolt), 1);
        this.land(bolt, true);
      } else {
        this.playLoop(boltLoopCue(String(bolt.id), bolt.cast.spell, flight.position, true));
      }
    }
  }

  dispose(): void {
    this.stops.forEach((stop) => stop());
    for (const bolt of this.bolts.splice(0)) {
      this.land(bolt, false);
    }
  }

  /** Stops the bolt's loop, with its impact when it reached the end of its flight. */
  private land(bolt: Bolt, impact: boolean): void {
    const landing = landingPoint(bolt.cast);
    this.playLoop(boltLoopCue(String(bolt.id), bolt.cast.spell, landing, false));
    if (!impact) {
      return;
    }
    const target = bolt.cast.targetId ? this.getState().players[bolt.cast.targetId]?.pos : null;
    for (const cue of impactCues(bolt.cast.spell, landing, target ? { ...target } : null)) {
      this.sfx.play(cue);
    }
  }

  private playLoop(cue: SoundCue | null): void {
    if (cue) {
      this.sfx.play(cue);
    }
  }
}
