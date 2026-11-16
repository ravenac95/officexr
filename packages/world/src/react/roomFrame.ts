import type {
  Actions,
  Bus,
  OfficeState,
  PlayerState,
  ProximityConstants,
  Store,
  TrackerOutput,
  Vec3,
} from '@officexr/sdk';
import type { JoystickKind, MoveIntent, ProximityInput, WorldDiff } from '@officexr/render-wasm';
import type { BotPool } from '../bot/BotPool.ts';
import { resolveCharacterTunables } from '../characters/resolve.ts';
import { feetPosition } from '../physics/rules.ts';
import type { CameraMode } from '../renderer/config.ts';
import { yawFromDirection } from '../renderer/lookControls.ts';
import type { AutoWalkSource, MoveInput } from '../renderer/moveIntent.ts';
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
  /** Sitting: drawn in the seat pose at this body root and yaw. */
  seat: { pos: Xyz; yaw: number } | null;
  /** Each increase of `count` is a bump, pushed along `normal` (XZ). */
  bump: { count: number; normal: [number, number] };
  /** The character's clip-speed tunables; at 1 a walk or run plays at the rate that keeps its feet planted. */
  animSpeeds: { idle: number; walk: number; run: number };
}

export interface PlayersInput {
  players: PlayerInput[];
  charRadius: number;
  playerSpeed: number;
  runSpeed: number;
  bumpEasingMs: number;
}

/** The calls one frame makes on its Bevy view. */
export interface RoomView {
  takeIntent(): MoveIntent;
  setInputEnabled(enabled: boolean): void;
  setStick(stick: [number, number]): void;
  /** Whether the view can show its Bevy stick. */
  setJoystickKind(kind: JoystickKind): boolean;
  setWorld(diff: WorldDiff): void;
  setPlayers(players: PlayersInput): void;
  setFollowTarget(pos: Xyz, yaw: number): void;
  setConversationFocus(point: Xyz | null): void;
  setProximity(proximity: ProximityInput): void;
}

export interface RoomFrameDeps {
  store: Store;
  actions: Actions;
  bus: Bus;
  bots?: BotPool;
  selfId: string;
  view: RoomView;
  /** Whether the page's DOM joystick walks: picked, or standing in for a view without a Bevy stick. */
  onDomJoystick?(dom: boolean): void;
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
  /** The DOM touch joystick, sent to the view while `joystickKind` is `'dom'`; keys win when held. */
  moveInput?: { readonly current: MoveInput };
  joystickKind: JoystickKind;
  autoWalk?: AutoWalkSource;
  seating?: SeatingSource;
  /** The character model `setPlayers` draws for a player: a built-in's name, or a model's path or URL. */
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
  private joystickKind: JoystickKind | null = null;
  private domJoystick = false;
  private wasMoving = false;
  private airborne = false;
  private proximity: TrackerOutput | null = null;
  private sentWorld: SentWorld | null = null;
  private worldSources: unknown[] = [];
  private readonly bumps = new Map<string, PlayerInput['bump']>();
  private readonly stopBumps: () => void;

  constructor(
    private readonly deps: RoomFrameDeps,
    options: RoomFrameOptions,
  ) {
    this.options = options;
    this.stopBumps = deps.bus.on('collision:char-bump', ({ selfId, normal }) => {
      this.bumps.set(selfId, { count: (this.bumps.get(selfId)?.count ?? 0) + 1, normal: [normal.x, normal.z] });
    });
  }

  dispose(): void {
    this.stopBumps();
  }

  step(dtSec: number, nowMs: number): void {
    const { view, bots } = this.deps;
    this.syncInputEnabled();
    this.syncJoystick();
    const intent = view.takeIntent();
    this.deps.onViewYaw?.(intent.yaw);
    this.simulate(intent, dtSec, nowMs);
    bots?.tick(dtSec * 1000);
    this.draw(nowMs);
  }

  private syncJoystick() {
    const { view } = this.deps;
    const { joystickKind, moveInput } = this.options;
    if (joystickKind !== this.joystickKind) {
      this.joystickKind = joystickKind;
      this.domJoystick = !view.setJoystickKind(joystickKind) || joystickKind === 'dom';
      this.deps.onDomJoystick?.(this.domJoystick);
    }
    if (this.domJoystick && moveInput) {
      view.setStick([moveInput.current.x, moveInput.current.z]);
    }
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
    const hasMoveInput = !freeFly && (intent.dir[0] !== 0 || intent.dir[1] !== 0);
    const deflection = hasMoveInput ? intent.deflection : 0;
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
    const running = !freeFly && intent.run;
    const speed = autoWalkDir
      ? baseSpeed
      : (running ? baseSpeed * runSpeedMultiplier : baseSpeed) * (deflection > 0 ? deflection : 1);
    const dir = hasMoveInput ? { x: intent.dir[0], z: intent.dir[1] } : (autoWalkDir ?? { x: 0, z: 0 });
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
    if (this.proximity) {
      // `setPlayers` leaves the first-person player out; its disc still shows at its feet.
      const undrawn =
        self && this.options.cameraMode === 'first-person'
          ? { [selfId]: xyz(feetPosition(self.pos, state.worldSettings.charRadius)) }
          : {};
      view.setProximity(
        proximityInput(this.proximity, state.worldConstants.proximity, {
          undrawn,
          selfId,
          // The fixed camera sits far above the floor, where stock bubbles would be specks.
          sizeMul: this.options.cameraMode === 'fixed' ? 5 : 1,
        }),
      );
    }
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
    for (const id of this.bumps.keys()) {
      if (!(id in state.players)) {
        this.bumps.delete(id);
      }
    }
    const players = Object.values(state.players)
      .filter((p) => p.id !== selfId || cameraMode !== 'first-person')
      .map((p): PlayerInput => {
        const isSelf = p.id === selfId;
        const pos = isSelf ? p.pos : sim ? sim.extrapolatePeer(p, nowMs) : extrapolatePos(p, nowMs);
        const tun = resolveCharacterTunables(p.avatar.model ?? 'default', ws, state.characterConfigs);
        return {
          id: p.id,
          character: this.options.characterOf(p),
          pos: xyz(pos),
          yaw: p.yaw,
          vel: xyz(p.vel),
          name: isSelf ? '' : p.name,
          airborne: isSelf ? this.airborne : p.isAirborne,
          seat: p.seat == null ? null : { pos: xyz(p.pos), yaw: p.yaw },
          bump: this.bumps.get(p.id) ?? { count: 0, normal: [0, 0] },
          animSpeeds: { idle: tun.idleAnimSpeed, walk: tun.walkAnimSpeed, run: tun.runAnimSpeed },
        };
      });
    return {
      players,
      charRadius: ws.charRadius,
      playerSpeed: ws.playerSpeed,
      runSpeed: ws.playerSpeed * ws.runSpeedMultiplier,
      bumpEasingMs: ws.bumpEasingMs,
    };
  }
}

function proximityInput(
  output: TrackerOutput,
  style: ProximityConstants,
  { undrawn, selfId, sizeMul }: { undrawn: Record<string, Xyz>; selfId: string; sizeMul: number },
): ProximityInput {
  const { discRadius, pulseSpeed, intensity, enteringColor, enteredColor, exitingColor } = style;
  const { meetingBorderInset, meetingBorderOutset, sparkleSpeed, sparkleFloatHeight, sparkleSize } = style;
  // Bubbles rise around both ends of the local player's approaches.
  const bubbles = new Set<string>();
  for (const [key, pair] of output.pairStates) {
    const [a, b] = key.split('|');
    if (pair === 'entering' && (a === selfId || b === selfId) && a && b) {
      bubbles.add(a).add(b);
    }
  }
  return {
    players: Object.fromEntries(output.playerStates),
    areas: output.areas.map((a) => ({
      id: a.id,
      center: [a.center.x, a.center.z],
      radius: a.radius,
      members: [...a.members],
    })),
    undrawn,
    bubbles: [...bubbles],
    style: {
      discRadius,
      pulseSpeed,
      intensity,
      enteringColor,
      enteredColor,
      exitingColor,
      meetingBorderInset,
      meetingBorderOutset,
      sparkleSpeed,
      sparkleFloatHeight,
      sparkleSize: sparkleSize * sizeMul,
    },
  };
}

