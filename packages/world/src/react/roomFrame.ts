import type { Actions, Bus, OfficeState, PlayerState, Store, TrackerOutput, Vec3 } from '@officexr/sdk';
import type { MoveIntent, WorldDiff } from '@officexr/render-wasm';
import type { BotPool } from '../bot/BotPool.ts';
import { resolveCharacterTunables } from '../characters/resolve.ts';
import { feetPosition } from '../physics/rules.ts';
import type { CameraMode } from '../renderer/config.ts';
import { yawFromDirection } from '../renderer/lookControls.ts';
import { RUN_DEFLECTION, type AutoWalkSource, type MoveInput } from '../renderer/moveIntent.ts';
import type { SeatingSource } from '../renderer/seated.ts';
import type { ExcludeInstance } from '../scenes/compile-map.ts';
import { extrapolatePos } from '../sim/extrapolate.ts';
import type { SimCore, SimPeer } from '../sim/SimCore.ts';
import { isTypingTarget } from './useWorldFocus.ts';
import { diffWorld, type BakedLayoutDiff, type SentWorld } from './worldDiff.ts';

type Xyz = [number, number, number];

/** One player as `setPlayers` takes it. */
export interface PlayerInput {
  id: string;
  character: string;
  pos: Xyz;
  yaw: number;
  vel: Xyz;
  /** Empty hides the name tag. */
  name: string;
  airborne: boolean;
}

export interface PlayersInput {
  players: PlayerInput[];
  charRadius: number;
  playerSpeed: number;
  runSpeed: number;
}

/** The calls one frame makes on its Bevy view. */
export interface RoomView {
  takeIntent(): MoveIntent;
  setInputEnabled(enabled: boolean): void;
  setWorld(diff: WorldDiff): void;
  setPlayers(players: PlayersInput): void;
  setFollowTarget(pos: Xyz, yaw: number): void;
  setConversationFocus(point: Xyz | null): void;
}

export interface RoomFrameDeps {
  store: Store;
  actions: Actions;
  bus: Bus;
  bots?: BotPool;
  selfId: string;
  view: RoomView;
  /** Receives the camera yaw each frame. */
  onViewYaw?(yaw: number): void;
}

/** What the frame reads from the React tree, refreshed every render. */
export interface RoomFrameOptions {
  /** The local player's simulation, once loaded. */
  sim: SimCore | null;
  cameraMode: CameraMode;
  /** From `useWorldFocus`; input is also off while a text field has focus. */
  worldFocused: boolean;
  /** False until the world is ready; the player is held where it is. */
  selfReady: boolean;
  spawnPoints: readonly Vec3[];
  /** Analog walking input, e.g. a touch joystick; keys win when held. */
  moveInput?: { readonly current: MoveInput };
  autoWalk?: AutoWalkSource;
  seating?: SeatingSource;
  /** The character model `setPlayers` draws for a player. */
  characterOf(player: PlayerState): string;
  kinds: readonly unknown[];
  bakedLayouts: readonly BakedLayoutDiff[];
  /** The instances a baked layout draws. */
  excludeInstance?: ExcludeInstance;
}

/** The warp detector's threshold: well above a frame of gravity or walking, below any real teleport. */
const WARP_DISTANCE_SQ = 0.5 * 0.5;

const xyz = (v: Vec3): Xyz => [v.x, v.y, v.z];

/**
 * One frame of the 3D play view, in order: the view's movement intent, the
 * local player's sim (peers, proximity and its bus events, then the step and
 * `setSelfPosition`), bots, and what the view draws: the world, the players,
 * the follow target and the conversation focus. The frame loop then ticks
 * the session.
 */
export class RoomFrame {
  options: RoomFrameOptions;
  private inputEnabled: boolean | null = null;
  private wasMoving = false;
  private airborne = false;
  private proximity: TrackerOutput | null = null;
  private sentWorld: SentWorld | null = null;
  private worldSources: unknown[] = [];

  constructor(
    private readonly deps: RoomFrameDeps,
    options: RoomFrameOptions,
  ) {
    this.options = options;
  }

  step(dtSec: number, nowMs: number): void {
    const { view, bots } = this.deps;
    this.syncInputEnabled();
    const intent = view.takeIntent();
    this.deps.onViewYaw?.(intent.yaw);
    this.simulate(intent, dtSec, nowMs);
    bots?.tick(dtSec * 1000);
    this.draw(nowMs);
  }

  private syncInputEnabled() {
    const focus = globalThis.document?.activeElement ?? null;
    const enabled = this.options.worldFocused && !isTypingTarget(focus);
    if (enabled !== this.inputEnabled) {
      this.inputEnabled = enabled;
      this.deps.view.setInputEnabled(enabled);
    }
  }

  // SRP violation: input, proximity, warp, seating and the step share one
  // frame's snapshot of store state, as SceneFrame's body did. Splitting
  // them means giving each its own sim stage.
  private simulate(intent: MoveIntent, dtSec: number, nowMs: number) {
    const { store, actions, bus, selfId } = this.deps;
    const { sim, autoWalk, seating } = this.options;
    // Free-fly flies the camera on the movement keys, and nothing walks the player.
    const freeFly = this.options.cameraMode === 'free-fly';
    const keyed = !freeFly && (intent.dir[0] !== 0 || intent.dir[1] !== 0);
    const stick = keyed || freeFly ? undefined : this.options.moveInput?.current;
    const deflection = stick ? Math.hypot(stick.x, stick.z) : 0;
    const hasMoveInput = keyed || deflection > 0;
    const jump = !freeFly && intent.jump;
    if (hasMoveInput) {
      autoWalk?.cancel();
    }
    const selfBeforeWalk = store.getState().players[selfId];
    const autoWalkDir =
      !hasMoveInput && !freeFly && selfBeforeWalk ? (autoWalk?.intent(selfBeforeWalk.pos) ?? null) : null;

    // Read after auto-walk, which can seat the player, so this frame's movement doesn't overwrite the seated pose.
    const state = store.getState();
    const self = state.players[selfId];
    const { worldSettings } = state;

    if (sim) {
      const peers: SimPeer[] = Object.entries(state.players)
        .filter(([id]) => id !== selfId)
        .map(([id, p]) => ({ id, pos: sim.extrapolatePeer(p, nowMs) }));
      sim.syncPeers(peers, worldSettings.charRadius);
      const proximity = sim.tickProximity(state.players, {
        voiceZones: state.voiceZones,
        selfId,
        settings: worldSettings,
        nowMs,
        dtSec,
      });
      for (const event of proximity.events) {
        bus.emit(event);
      }
      this.proximity = proximity;
    }

    const held = !this.options.selfReady;
    if (held || !self) {
      return;
    }
    if (self.seat != null) {
      if (hasMoveInput || jump || autoWalkDir !== null) {
        seating?.stand();
      }
      sim?.teleport(store.getState().players[selfId]?.pos ?? self.pos);
      this.airborne = false;
      this.wasMoving = false;
      return;
    }
    if (!sim) {
      return;
    }
    // A store position far from the sim's is a warp (a map-switch spawn, a
    // test's `setState`): teleport there and skip this frame's step, which
    // would move on from the old position.
    const before = sim.position();
    const dx0 = self.pos.x - before.x;
    const dy0 = self.pos.y - before.y;
    const dz0 = self.pos.z - before.z;
    if (dx0 * dx0 + dy0 * dy0 + dz0 * dz0 > WARP_DISTANCE_SQ) {
      sim.teleport(self.pos);
      this.airborne = false;
      this.wasMoving = false;
      return;
    }

    const { playerSpeed: baseSpeed, runSpeedMultiplier } = resolveCharacterTunables(
      self.avatar.model ?? 'default',
      worldSettings,
      state.characterConfigs,
    );
    const running = (!freeFly && intent.run) || deflection >= RUN_DEFLECTION;
    const speed = autoWalkDir
      ? baseSpeed
      : (running ? baseSpeed * runSpeedMultiplier : baseSpeed) * (deflection > 0 ? deflection : 1);
    const dir = keyed
      ? { x: intent.dir[0], z: intent.dir[1] }
      : stick && deflection > 0
        ? stickDirection(stick, intent.yaw)
        : (autoWalkDir ?? { x: 0, z: 0 });
    const moving = dir.x !== 0 || dir.z !== 0;

    const result = sim.step({
      dtSec,
      nowMs,
      dir,
      speed,
      running: running && !autoWalkDir,
      yaw: moving ? yawFromDirection(dir) : self.yaw,
      jump,
      settings: worldSettings,
      spawnPoints: this.options.spawnPoints,
    });
    this.airborne = result.isAirborne;
    // Bilateral: `normal` points out of the peer, the way self is pushed.
    for (const { otherId, normal } of result.bumps) {
      bus.emit({ kind: 'collision:char-bump', selfId, otherId, normal });
      bus.emit({ kind: 'collision:char-bump', selfId: otherId, otherId: selfId, normal: { x: -normal.x, z: -normal.z } });
    }
    actions.setSelfPosition(result.newPos, result.broadcastVel, result.broadcastYaw, result.isAirborne);
    // On the moving → idle edge, zero the velocity once so peers see the stop.
    if (!result.moved && this.wasMoving) {
      actions.setSelfPosition(before, { x: 0, y: 0, z: 0 }, self.yaw, false);
    }
    this.wasMoving = result.moved;
  }

  private draw(nowMs: number) {
    const { store, view, selfId } = this.deps;
    const state = store.getState();
    this.syncWorld(state);
    view.setPlayers(this.playersInput(state, nowMs));
    const self = state.players[selfId];
    if (self) {
      view.setFollowTarget(xyz(self.pos), self.yaw);
    }
    const focus = this.proximity?.conversationFocus;
    view.setConversationFocus(
      focus && self ? [focus.x, feetPosition(self.pos, state.worldSettings.charRadius).y, focus.z] : null,
    );
  }

  /** Sends the world when one of its sources changed since the last frame. */
  private syncWorld(state: OfficeState) {
    const { kinds, bakedLayouts, excludeInstance } = this.options;
    const { lighting, background } = state.worldConstants;
    const sources = [state.worldObjects, kinds, bakedLayouts, excludeInstance, lighting, background];
    if (sources.every((s, i) => s === this.worldSources[i])) {
      return;
    }
    const { instances } = state.worldObjects;
    const { diff, sent } = diffWorld(this.sentWorld, {
      instances: excludeInstance ? instances.filter((i) => !excludeInstance(i)) : instances,
      kinds,
      lighting,
      background,
      bakedLayouts,
    });
    if (diff) {
      this.deps.view.setWorld(diff);
    }
    // Only once it landed, so a diff that throws is tried again next frame.
    this.worldSources = sources;
    this.sentWorld = sent;
  }

  private playersInput(state: OfficeState, nowMs: number): PlayersInput {
    const { selfId } = this.deps;
    const { sim, cameraMode } = this.options;
    const ws = state.worldSettings;
    // ponytail(OSO-7939): seated players are drawn standing; Bevy avatars have no sit pose yet.
    const players = Object.values(state.players)
      .filter((p) => p.id !== selfId || cameraMode !== 'first-person')
      .map((p): PlayerInput => {
        const isSelf = p.id === selfId;
        const pos = isSelf ? p.pos : sim ? sim.extrapolatePeer(p, nowMs) : extrapolatePos(p, nowMs);
        return {
          id: p.id,
          character: this.options.characterOf(p),
          pos: xyz(pos),
          yaw: p.yaw,
          vel: xyz(p.vel),
          name: isSelf ? '' : p.name,
          airborne: isSelf ? this.airborne : p.isAirborne,
        };
      });
    return {
      players,
      charRadius: ws.charRadius,
      playerSpeed: ws.playerSpeed,
      runSpeed: ws.playerSpeed * ws.runSpeedMultiplier,
    };
  }
}

/** A stick's deflection turned into world space by the camera's yaw; unit length. */
function stickDirection(stick: MoveInput, yaw: number): { x: number; z: number } {
  const x = -Math.sin(yaw) * stick.z + Math.cos(yaw) * stick.x;
  const z = -Math.cos(yaw) * stick.z - Math.sin(yaw) * stick.x;
  const len = Math.hypot(x, z);
  return { x: x / len, z: z / len };
}
