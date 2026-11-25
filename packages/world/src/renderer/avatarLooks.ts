import { currentVisual, type Bus, type OfficeState, type PlayerState } from '@officexr/sdk';
import { POKE_TURN_S, yawToward } from '../motion/poke.ts';
import { POSE_VISUALS } from '../motion/poseVisual.ts';
import { leanOf, RIDE_STRATEGIES } from '../motion/ride.ts';
import { AT_REST, BODY_CENTRE_M, floatSway, floatTarget, stepBuoyancy, stepPitch, swimStateOf, type Buoyancy } from '../motion/swim.ts';
import { waterAt, type WaterVolume } from '../motion/water.ts';
import { feetPosition } from '../physics/rules.ts';
import { hexColor } from './hexColor.ts';
import { wrapAngle } from './lookControls.ts';

/** `setPlayers`' per-player look fields, as `Players.tsx` derived them each frame. */
export interface AvatarLook {
  /** A pose's rig clip, over the motion's. */
  clip: { name: string; speed: number } | null;
  /** The body's offset from its feet, in the avatar's frame. */
  body: { lift: number; forward: number; pitch: number; roll: number; scale: number };
  /** The ride's model under the rider. */
  vehicle: string | null;
  wand: boolean;
  /** Pokes this player has made; each increase points once. */
  pokes: number;
  /** Spells this player has cast; each increase waves the wand once. */
  casts: number;
  tint: { color: string; strength: number; glow: boolean } | null;
  aura: 'smoke' | 'sparkles' | null;
}

interface Motion {
  buoyancy: Buoyancy;
  pitch: number | null;
  lastYaw: number | null;
  yawRate: number;
}

interface Flinch {
  yaw: number;
  untilMs: number;
}

/** A bump a poke gave a player: `count` of them, the last pushed along `normal` at `atMs`. */
export interface PokeBump {
  count: number;
  normal: [number, number];
  atMs: number;
}

const RATE_SMOOTHING = 10;

/**
 * The per-player, per-frame look of each avatar that `Players.tsx` derived in its `useFrame`:
 * a pose's clip, buoyancy, pitch and lean; a spell's tint, lift, scale and aura; the wand; and the
 * poke and cast counters with a poke's flinch. Headless: the frame hands the result to Bevy.
 */
export class AvatarLooks {
  private readonly motion = new Map<string, Motion>();
  private readonly pokes = new Map<string, number>();
  private readonly casts = new Map<string, number>();
  private readonly flinches = new Map<string, Flinch>();
  private readonly pokeBumps = new Map<string, PokeBump>();
  private readonly stops: (() => void)[];

  constructor(
    bus: Bus,
    private readonly selfId: string,
    private readonly store: { getState(): OfficeState },
    private readonly now: () => number = () => performance.now(),
  ) {
    this.stops = [
      bus.on('spell:cast', ({ casterId }) => {
        this.casts.set(casterId, (this.casts.get(casterId) ?? 0) + 1);
      }),
      bus.on('poke:received', ({ fromId, targetId }) => this.onPoke(fromId, targetId)),
    ];
  }

  dispose(): void {
    for (const stop of this.stops) {
      stop();
    }
  }

  private onPoke(fromId: string, targetId: string | null) {
    this.pokes.set(fromId, (this.pokes.get(fromId) ?? 0) + 1);
    const players = this.store.getState().players;
    const from = players[fromId];
    const target = targetId ? players[targetId] : undefined;
    if (!targetId || !from || !target) {
      return;
    }
    const dx = target.pos.x - from.pos.x;
    const dz = target.pos.z - from.pos.z;
    const len = Math.hypot(dx, dz) || 1;
    const now = this.now();
    this.pokeBumps.set(targetId, {
      count: (this.pokeBumps.get(targetId)?.count ?? 0) + 1,
      normal: [dx / len, dz / len],
      atMs: now,
    });
    if (targetId !== this.selfId) {
      this.flinches.set(targetId, { yaw: yawToward(target.pos, from.pos), untilMs: now + POKE_TURN_S * 1000 });
    }
  }

  /** The yaw `player` is drawn at: toward whoever just poked it, else its own. */
  yawOf(player: PlayerState, nowMs: number): number {
    const flinch = this.flinches.get(player.id);
    if (flinch && nowMs > flinch.untilMs) {
      this.flinches.delete(player.id);
    }
    return flinch && nowMs <= flinch.untilMs ? flinch.yaw : player.yaw;
  }

  pokeBumpOf(id: string): PokeBump | null {
    return this.pokeBumps.get(id) ?? null;
  }

  /** Forgets players who left. */
  prune(players: OfficeState['players']): void {
    for (const map of [this.motion, this.pokes, this.casts, this.flinches, this.pokeBumps]) {
      for (const id of map.keys()) {
        if (!(id in players)) {
          map.delete(id);
        }
      }
    }
  }

  look(
    player: PlayerState,
    state: Pick<OfficeState, 'spellEffects' | 'worldSettings'>,
    water: readonly WaterVolume[],
    nowMs: number,
    dtSec: number,
  ): AvatarLook {
    const motion = this.motion.get(player.id) ?? { buoyancy: AT_REST, pitch: null, lastYaw: null, yawRate: 0 };
    this.motion.set(player.id, motion);
    const speed = Math.hypot(player.vel.x, player.vel.z);
    const yawDelta = wrapAngle(player.yaw - (motion.lastYaw ?? player.yaw));
    motion.lastYaw = player.yaw;
    motion.yawRate += ((dtSec > 0 ? yawDelta / dtSec : 0) - motion.yawRate) * Math.min(1, RATE_SMOOTHING * dtSec);

    const visual = player.pose ? POSE_VISUALS[player.pose](speed) : null;
    const body = { lift: 0, forward: 0, pitch: 0, roll: 0, scale: 1 };
    if (visual) {
      body.lift = visual.lift;
      if (visual.floats) {
        const volume = waterAt(water, player.pos.x, player.pos.z);
        const feetY = feetPosition(player.pos, state.worldSettings.charRadius).y;
        const target = volume ? floatTarget(volume.surfaceY, feetY, swimStateOf(speed), nowMs / 1000) : 0;
        motion.buoyancy = stepBuoyancy(motion.buoyancy, target, dtSec);
        body.lift = motion.buoyancy.lift;
      } else {
        motion.buoyancy = AT_REST;
      }
      motion.pitch = stepPitch(motion.pitch ?? visual.pitch, visual.pitch, dtSec);
      body.pitch = motion.pitch;
      body.forward = visual.floats ? -Math.sin(motion.pitch) * BODY_CENTRE_M : 0;
      body.roll = visual.vehicle
        ? leanOf(visual.vehicle, motion.yawRate, speed)
        : visual.floats && swimStateOf(speed) === 'tread'
          ? floatSway(nowMs / 1000)
          : 0;
    } else {
      motion.buoyancy = AT_REST;
      motion.pitch = null;
    }

    const spell = currentVisual(state.spellEffects, player.id, nowMs);
    body.lift += spell?.lift ?? 0;
    body.scale = spell?.scale ?? 1;
    const tint = spell?.tint ? hexColor(spell.tint) : null;
    return {
      clip: visual ? { name: visual.clip, speed: Math.round(visual.timeScale * 10) / 10 } : null,
      body,
      vehicle: visual?.vehicle ? RIDE_STRATEGIES[visual.vehicle].model : null,
      wand: player.held === 'wand',
      pokes: this.pokes.get(player.id) ?? 0,
      casts: this.casts.get(player.id) ?? 0,
      tint: spell && tint ? { color: tint, strength: spell.tintStrength, glow: spell.sparkles } : null,
      aura: spell?.smoke ? 'smoke' : spell?.sparkles ? 'sparkles' : null,
    };
  }
}
