import React, { useEffect, useRef, useState } from 'react';
import * as THREE from 'three';
import { useFrame } from '@react-three/fiber';
import type { Bus, OfficeState, PlayerId, Store } from '@officexr/sdk';
import { Adventurer, ANIMATION_URLS, type MotionState } from './Adventurer.tsx';
import { assertNever } from './assertNever.ts';
import type { CameraMode } from './config.ts';
import { PlayerNameTag } from './PlayerNameTag.tsx';
import { PlayerPingBell, shouldShowPingBell } from './PlayerPingBell.tsx';
import { pickCharacterForPlayer, resolvePlayerModel, type CatalogCharacters, type PlayerModel } from '../characters/registry.ts';
import { useCatalogCharacters } from '../react/application-context.tsx';
import { ModelWithFallback } from './ModelWithFallback.tsx';
import { SwapWhenLoaded } from './SwapWhenLoaded.tsx';
import { resolveCharacterTunables } from '../characters/resolve.ts';
import { extrapolatePos } from '../sim/extrapolate.ts';
import { AVATAR_YAW_OFFSET, BODY_Y } from '../physics/rules.ts';

interface PlayersProps {
  store: Store;
  bus: Bus;
  selfId: string;
  cameraMode: CameraMode;
  /** Mutated each frame so the camera rig reads the latest local position. */
  selfPosRef: React.MutableRefObject<THREE.Vector3>;
  /** When true, every character is rendered at its bind pose with no
   * animation mixer activity (no idle clip, no bump reactions, no
   * timeScale updates). Used by the Mugshot mode to produce a
   * deterministic frame for snapshot testing. */
  paused?: boolean;
  /** When false, the self player is not rendered. Default true. */
  selfReady?: boolean;
  /** Ref mutated each frame by SceneFrame to reflect local player airborne
   * state. Used to override motion to 'jumping' for the self avatar without
   * a React re-render. Ref is created in Scene.tsx and threaded to both
   * SceneFrame and Players. */
  isAirborneRef: React.MutableRefObject<boolean>;
  /** Hides the self avatar, as in first-person, while an immersive XR session is presenting. */
  xrPresenting: boolean;
  onPingPlayer?: (id: PlayerId) => void;
}

interface BumpState {
  /** ms since unix-epoch-ish timestamp when the bump was registered. */
  startMs: number;
  /** Push direction (away from the other character). */
  normal: { x: number; z: number };
}

interface PlayerEntry {
  id: string;
  character: string;
  model: PlayerModel;
}

const WALK_THRESHOLD = 0.05;

const playerEntries = (state: OfficeState, catalog: CatalogCharacters): PlayerEntry[] =>
  Object.entries(state.players).map(([id, player]) => ({
    id,
    character: pickCharacterForPlayer(id, player.avatar.model, catalog),
    model: resolvePlayerModel(id, player.avatar, state.worldConstants.ghostAvatar, catalog),
  }));

const sameEntries = (a: PlayerEntry[], b: PlayerEntry[]) =>
  a.length === b.length &&
  a.every(
    (p, i) =>
      p.id === b[i].id &&
      p.character === b[i].character &&
      p.model.url === b[i].model.url &&
      p.model.ghost === b[i].model.ghost,
  );

/** Rotate `from` toward `to` by at most `maxStep` radians along the shortest arc. */
function stepTowardAngle(from: number, to: number, maxStep: number): number {
  let delta = ((to - from + Math.PI) % (Math.PI * 2)) - Math.PI;
  if (delta < -Math.PI) delta += Math.PI * 2;
  if (Math.abs(delta) <= maxStep) return to;
  return from + Math.sign(delta) * maxStep;
}

/**
 * Renders one Adventurer per player in a root group at its feet, moved each
 * frame to the store position (peers extrapolated). Visual offsets (bump
 * easing, smooth yaw turn) go on an inner `<group>` so the root stays put.
 */
export function Players({
  store,
  bus,
  selfId,
  cameraMode,
  selfPosRef,
  paused,
  isAirborneRef,
  xrPresenting,
  selfReady = true,
  onPingPlayer,
}: PlayersProps) {
  const [worldSettings, setWorldSettings] = useState(
    () => store.getState().worldSettings,
  );
  useEffect(() => {
    return store.subscribe(
      (s) => s.worldSettings,
      (next) => setWorldSettings(next),
    );
  }, [store]);
  const {
    idleAnimSpeed,
    walkAnimSpeed,
    runAnimSpeed,
    turnSpeed,
    playerSpeed,
    runSpeedMultiplier,
    charRadius,
  } = worldSettings;
  const runSpeed = playerSpeed * runSpeedMultiplier;
  const catalog = useCatalogCharacters();
  const [players, setPlayers] = useState<PlayerEntry[]>(() =>
    playerEntries(store.getState(), catalog),
  );

  // Re-render only when the player set or someone's character changes, not on movement.
  useEffect(() => {
    const entries = (s: OfficeState) => playerEntries(s, catalog);
    // The catalog can load after the store, so re-resolve against the new one.
    setPlayers((prev) => {
      const next = entries(store.getState());
      return sameEntries(prev, next) ? prev : next;
    });
    return store.subscribe(entries, setPlayers, sameEntries);
  }, [store, catalog]);

  // Root group per player, at its feet; peers and self alike follow the store each frame.
  const rootRefs = useRef<Map<string, THREE.Group>>(new Map());
  // Inner <group> per player — visual yaw + bump offset live here.
  const groupRefs = useRef<Map<string, THREE.Group>>(new Map());
  const motionState = useRef<Map<string, MotionState>>(new Map());
  const currentYaw = useRef<Map<string, number>>(new Map());
  const [motionByPlayer, setMotionByPlayer] = useState<
    Record<string, MotionState>
  >({});
  const [animScaleByPlayer, setAnimScaleByPlayer] = useState<
    Record<string, number>
  >({});
  const animScaleQuant = useRef<Map<string, number>>(new Map());

  const bumps = useRef<Map<string, BumpState>>(new Map());
  const [bumpCounters, setBumpCounters] = useState<Record<string, number>>(
    {},
  );
  useEffect(() => {
    return bus.on('collision:char-bump', (evt) => {
      bumps.current.set(evt.selfId, {
        startMs: performance.now(),
        normal: evt.normal,
      });
      setBumpCounters((prev) => ({
        ...prev,
        [evt.selfId]: (prev[evt.selfId] ?? 0) + 1,
      }));
    });
  }, [bus]);

  useFrame((_, dt) => {
    const state = store.getState();
    const now = performance.now();
    let motionChanged = false;
    const nextMotion: Record<string, MotionState> = {};
    let animScaleChanged = false;
    const nextAnimScales: Record<string, number> = {};
    const runThreshold = (playerSpeed + runSpeed) / 2;
    const WALK_ENTER_SQ = (WALK_THRESHOLD * 1.5) * (WALK_THRESHOLD * 1.5);
    const WALK_EXIT_SQ = WALK_THRESHOLD * WALK_THRESHOLD;
    const RUN_ENTER_SQ = (runThreshold * 1.05) * (runThreshold * 1.05);
    const RUN_EXIT_SQ = (runThreshold * 0.95) * (runThreshold * 0.95);

    for (const [id, player] of Object.entries(state.players)) {
      const grp = groupRefs.current.get(id);
      const body = rootRefs.current.get(id);
      if (!grp || !body) continue;

      // Extrapolate non-self players forward by their broadcast velocity to
      // smooth between rate-limited network updates (30 Hz broadcast →
      // 60 Hz render).
      const renderPos = extrapolatePos(player, now);
      body.position.set(renderPos.x, renderPos.y, renderPos.z);

      // Bump easing — visual-only offset on the inner group.
      const bump = bumps.current.get(id);
      let bumpX = 0;
      let bumpZ = 0;
      if (bump) {
        const elapsed = now - bump.startMs;
        const dur = worldSettings.bumpEasingMs;
        if (elapsed >= dur) {
          bumps.current.delete(id);
        } else {
          const t = elapsed / dur;
          const decay = Math.exp(-3.2 * t);
          const wave = Math.cos(2 * Math.PI * 1.1 * t);
          const env = decay * wave;
          const kick = 0.16;
          bumpX = bump.normal.x * env * kick;
          bumpZ = bump.normal.z * env * kick;
        }
      }
      // Inner group sits `BODY_Y - charRadius` above the root, so the model's
      // origin (its feet) lands on the standing surface the sim resolves at the ball's bottom.
      grp.position.set(bumpX, BODY_Y - charRadius, bumpZ);

      const targetYaw = (player.yaw ?? 0) + AVATAR_YAW_OFFSET;
      const prev = currentYaw.current.get(id) ?? targetYaw;
      grp.rotation.y = stepTowardAngle(prev, targetYaw, turnSpeed * dt);
      currentYaw.current.set(id, grp.rotation.y);

      const v = player.vel;
      const speedSq = v.x * v.x + v.z * v.z;
      const previous = motionState.current.get(id) ?? 'idle';
      // Seat and airborne are overrides below, not locomotion tiers, so hysteresis restarts from idle.
      const previousTier = previous === 'sitting' || previous === 'jumping' ? 'idle' : previous;
      let motion: MotionState =
        previousTier === 'idle'
          ? speedSq >= RUN_ENTER_SQ
            ? 'running'
            : speedSq >= WALK_ENTER_SQ
              ? 'walking'
              : 'idle'
          : previousTier === 'walking'
            ? speedSq >= RUN_ENTER_SQ
              ? 'running'
              : speedSq <= WALK_EXIT_SQ
                ? 'idle'
                : 'walking'
            : previousTier === 'running'
              ? speedSq <= WALK_EXIT_SQ
                ? 'idle'
                : speedSq <= RUN_EXIT_SQ
                  ? 'walking'
                  : 'running'
              : assertNever(previousTier);
      // Airborne override: if the player is in the air, force 'jumping'
      // regardless of horizontal speed. Driven by isAirborneRef for the
      // local player (frame-accurate, no re-render overhead); by
      // player.isAirborne for peers (broadcast once per state change via
      // presence:position).
      const playerIsAirborne =
        id === selfId
          ? isAirborneRef.current
          : (state.players[id]?.isAirborne ?? false);

      if (playerIsAirborne) {
        motion = 'jumping';
      }
      if (player.seat != null) {
        motion = 'sitting';
      }

      nextMotion[id] = motion;
      if (previous !== motion) {
        motionState.current.set(id, motion);
        motionChanged = true;
      }

      const speed = Math.sqrt(speedSq);
      const denom = motion === 'running' ? runSpeed : playerSpeed;
      const ratio = denom > 0 ? speed / denom : 0;
      const quantised = Math.round(Math.max(0, Math.min(1, ratio)) * 10) / 10;
      if (animScaleQuant.current.get(id) !== quantised) {
        animScaleQuant.current.set(id, quantised);
        animScaleChanged = true;
      }
      nextAnimScales[id] = quantised;

      if (id === selfId) {
        selfPosRef.current.copy(renderPos);
      }
    }

    if (motionChanged) setMotionByPlayer(nextMotion);
    if (animScaleChanged) setAnimScaleByPlayer(nextAnimScales);
  });

  return (
    <>
      {players.filter((p) => selfReady || p.id !== selfId).map((p) => {
        const isSelf = p.id === selfId;
        // Seeded at the current position so the avatar isn't drawn at the origin before the first frame.
        const spawn = store.getState().players[p.id]?.pos ?? { x: 0, y: 0, z: 0 };
        return (
          <group
            key={p.id}
            position={[spawn.x, spawn.y, spawn.z]}
            ref={(g: THREE.Group | null) => {
              if (g) {
                rootRefs.current.set(p.id, g);
              } else {
                rootRefs.current.delete(p.id);
              }
            }}
          >
            <group
              position={[0, BODY_Y - charRadius, 0]}
              ref={(g: THREE.Group | null) => {
                if (g) groupRefs.current.set(p.id, g);
                else groupRefs.current.delete(p.id);
              }}
            >
              <SwapWhenLoaded
                value={p.model}
                keyOf={modelKey}
                urlsOf={modelUrls}
                render={(model) => (
                  <PerPlayerAdventurer
                    // Remount on a character change so the animation mixer binds to the new model's bones.
                    key={modelKey(model)}
                    playerId={p.id}
                    model={model}
                    motion={motionByPlayer[p.id] ?? 'idle'}
                    invisible={isSelf && (cameraMode === 'first-person' || xrPresenting)}
                    animScale={animScaleByPlayer[p.id] ?? 1}
                    bumpCounter={bumpCounters[p.id] ?? 0}
                    store={store}
                    fallbackIdleSpeed={idleAnimSpeed}
                    fallbackWalkSpeed={walkAnimSpeed}
                    fallbackRunSpeed={runAnimSpeed}
                    paused={paused}
                  />
                )}
              />
              <PlayerNameTag
                store={store}
                playerId={p.id}
                isSelf={isSelf}
                paused={paused ?? false}
              />
              {onPingPlayer && shouldShowPingBell(onPingPlayer, isSelf, cameraMode, paused ?? false) && (
                <PlayerPingBell store={store} playerId={p.id} radius={charRadius} onPing={onPingPlayer} />
              )}
            </group>
          </group>
        );
      })}
    </>
  );
}

const modelKey = (m: PlayerModel) => `${m.url}|${m.ghost}`;
const modelUrls = (m: PlayerModel) => [m.url, ...ANIMATION_URLS];

/**
 * Per-player Adventurer wrapper that resolves animation speeds against
 * the player's broadcast `avatar.model`. Reads `characterConfigs` via a
 * narrow store subscription so a tuning change in CharacterMode flows
 * through to the right avatar's clip timeScale without re-rendering
 * every avatar.
 *
 * The fallback values are the world-level defaults (so an unconfigured
 * model behaves exactly as before this split).
 */
function PerPlayerAdventurer(props: {
  playerId: string;
  model: PlayerModel;
  motion: MotionState;
  invisible: boolean;
  animScale: number;
  bumpCounter: number;
  store: Store;
  fallbackIdleSpeed: number;
  fallbackWalkSpeed: number;
  fallbackRunSpeed: number;
  paused?: boolean;
}) {
  const {
    playerId,
    model,
    motion,
    invisible,
    animScale,
    bumpCounter,
    store,
    paused,
  } = props;
  const [tunedSpeeds, setTunedSpeeds] = useState(() => readSpeeds(store, playerId));
  useEffect(() => {
    return store.subscribe(
      (s) => ({
        model: s.players[playerId]?.avatar.model ?? 'default',
        ws: s.worldSettings,
        cc: s.characterConfigs,
      }),
      (next) => {
        const tun = resolveCharacterTunables(next.model, next.ws, next.cc);
        setTunedSpeeds({
          idle: tun.idleAnimSpeed,
          walk: tun.walkAnimSpeed,
          run: tun.runAnimSpeed,
        });
      },
      (a, b) =>
        a.model === b.model && a.ws === b.ws && a.cc === b.cc,
    );
  }, [store, playerId]);
  const adventurer = (url: string) => (
    <Adventurer
      url={url}
      ghost={model.ghost}
      motion={motion}
      invisible={invisible}
      idleSpeed={tunedSpeeds.idle}
      walkSpeed={tunedSpeeds.walk * animScale}
      runSpeed={tunedSpeeds.run * animScale}
      bumpCounter={bumpCounter}
      paused={paused}
    />
  );
  return <ModelWithFallback url={model.url} fallbackUrl={model.fallbackUrl} render={adventurer} />;
}

function readSpeeds(store: Store, playerId: string) {
  const s = store.getState();
  const model = s.players[playerId]?.avatar.model ?? 'default';
  const tun = resolveCharacterTunables(model, s.worldSettings, s.characterConfigs);
  return { idle: tun.idleAnimSpeed, walk: tun.walkAnimSpeed, run: tun.runAnimSpeed };
}
