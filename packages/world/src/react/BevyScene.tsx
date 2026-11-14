import React, { useEffect, useEffectEvent, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { CameraConfig, RendererModule, ViewId } from '@officexr/render-wasm';
import type { Actions, Bus, PlayerState, RuleRegistry, SnapshotHandshake, Store, SyncEngine, Vec3, WorldConstants } from '@officexr/sdk';
import type { BotPool } from '../bot/BotPool.ts';
import { pickCharacterForPlayer } from '../characters/registry.ts';
import type { CameraMode } from '../renderer/config.ts';
import type { FrameStatsSample } from '../renderer/FrameStatsProbe.tsx';
import type { AutoWalkSource, MoveInput } from '../renderer/moveIntent.ts';
import { placementTransform } from '../renderer/placement-transform.ts';
import type { SeatingSource } from '../renderer/seated.ts';
import { useBakedColliders, type SceneBakedLayout } from '../renderer/useBakedColliders.ts';
import { useStoreWorldObjects } from '../renderer/useStoreWorldObjects.ts';
import { excludeLayoutInstances } from '../scenes/compile-map.ts';
import type { SimBakedColliders, SimCore } from '../sim/SimCore.ts';
import { useApplication, useCatalog, useCatalogCharacters } from './application-context.tsx';
import { bootBevy, playCanvas } from './bevyRenderer.ts';
import { FrameLoop } from './frameLoop.ts';
import { RoomFrame, type RoomView } from './roomFrame.ts';
import { useCreateSim, type SimImplementation } from './useCreateSim.ts';
import { useStoreSelector } from './useStoreSelector.ts';
import { hasCommandModifier, isTypingTarget } from './useWorldFocus.ts';
import type { ViewYaw } from './viewYaw.tsx';
import type { BakedLayoutDiff } from './worldDiff.ts';

export interface BevySceneProps {
  store: Store;
  actions: Actions;
  rules: RuleRegistry;
  bus: Bus;
  sync: SyncEngine;
  handshake: SnapshotHandshake;
  bots?: BotPool;
  selfId: string;
  cameraMode: CameraMode;
  invertLook?: boolean;
  view: WorldConstants;
  worldFocused: boolean;
  moveInputRef?: React.RefObject<MoveInput>;
  autoWalk?: AutoWalkSource;
  seating?: SeatingSource;
  spawnPoints?: readonly Vec3[];
  /** Drawn from their baked GLBs; their rooms' store instances are left out of the view and the sim. */
  bakedLayouts?: readonly SceneBakedLayout[];
  /** False holds the self player where it is, until the world is ready. */
  selfReady: boolean;
  sim?: SimImplementation;
  onFrameStats?: (sample: FrameStatsSample | null) => void;
  viewYaw?: ViewYaw;
  /** The share of the view's models loaded so far, 0–100, until they all have. */
  onLoadProgress?: (percent: number) => void;
  /** Once per mount, when every model the view draws has loaded. */
  onModelsReady?: () => void;
  onCollidersReady?: (roomInstanceId: string) => void;
  onError?: (error: Error) => void;
}

interface BevySentinels {
  /** The play view has loaded every model; for Playwright. */
  __officexrBevyReady?: boolean;
}

const STATS_INTERVAL_MS = 500;
const SCROLL_KEYS = new Set(['Space', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']);
const NO_SPAWNS: readonly Vec3[] = [];

const selectCharRadius = (s: { worldSettings: { charRadius: number } }) => s.worldSettings.charRadius;

/** The URL Bevy loads a baked layout from: absolute, as Bevy resolves relative paths against the model origin. */
// ponytail: no bake version in the URL, so Bevy keeps drawing a layout re-baked mid-session until a reload.
function bakedLayoutDiff(p: SceneBakedLayout): BakedLayoutDiff {
  const path = p.gltfPath.endsWith('.glb') ? p.gltfPath : `${p.gltfPath}.glb`;
  return { gltfPath: new URL(path, window.location.href).href, position: p.position, rotationY: p.rotationY };
}

function cameraConfig(c: WorldConstants, charRadius: number, invertLook: boolean): CameraConfig {
  const f = c.fixedCamera;
  return {
    // Field by field: the controls reject keys they don't know.
    fixed: {
      azimuthDeg: f.azimuthDeg,
      pitchDeg: f.pitchDeg,
      height: f.height,
      maxOnScreenFrac: f.maxOnScreenFrac,
      minOnScreenFrac: f.minOnScreenFrac,
      lateralFrac: f.lateralFrac,
      fov: f.fov,
      movementYawOffsetDeg: f.movementYawOffsetDeg,
    },
    conversationDistance: c.proximity.conversationDistance,
    conversationHeight: c.proximity.conversationHeight,
    invertLook,
    charRadius,
  };
}

function roomView(wasm: RendererModule, view: ViewId): RoomView {
  return {
    takeIntent: () => wasm.takeIntent(view),
    setInputEnabled: (enabled) => wasm.setInputEnabled(view, enabled),
    setWorld: (diff) => void wasm.setWorld(view, JSON.stringify(diff)),
    setPlayers: (players) => wasm.setPlayers(view, players),
    setFollowTarget: (pos, yaw) => wasm.setFollowTarget(view, pos, yaw),
    setConversationFocus: (point) => wasm.setConversationFocus(view, point),
  };
}

/**
 * The 3D play view, drawn by Bevy into the page's play canvas: a
 * `FrameLoop` steps the local player's sim from the view's movement intent
 * and feeds the view the world and the players. Renders into the caller's
 * positioned container.
 */
export function BevyScene(props: BevySceneProps) {
  const { store, selfId, sync, seating, bakedLayouts } = props;
  const container = useRef<HTMLDivElement>(null);
  const [bevy, setBevy] = useState<{ wasm: RendererModule; view: ViewId } | null>(null);
  const reportError = useEffectEvent((error: Error) => props.onError?.(error));

  useEffect(() => {
    const canvas = playCanvas();
    container.current?.append(canvas);
    let cancelled = false;
    let mounted: { wasm: RendererModule; view: ViewId } | null = null;
    bootBevy(canvas)
      .then((wasm) => {
        if (!cancelled) {
          mounted = { wasm, view: wasm.createView(canvas, 'world') };
          setBevy(mounted);
        }
      })
      .catch((error: Error) => reportError(error));
    return () => {
      cancelled = true;
      if (mounted) {
        mounted.wasm.destroyView(mounted.view);
      }
      canvas.remove();
      delete (window as BevySentinels).__officexrBevyReady;
    };
  }, []);

  // Broadcast the world once mounted: the store's change diffs only send what changes, so a bot
  // that joined on the SDK defaults would otherwise never hear a map that matches them.
  useEffect(() => {
    sync.broadcastWorldState();
  }, [sync]);

  const createSim = useCreateSim(props.sim ?? 'ts', props.onError);
  const [sim, setSim] = useState<SimCore | null>(null);
  useEffect(() => {
    if (!createSim) {
      return;
    }
    const created = createSim(store.getState().worldSettings.charRadius);
    const selfPos = store.getState().players[selfId]?.pos;
    if (selfPos) {
      created.teleport(selfPos);
    }
    setSim(created);
    return () => {
      setSim(null);
      created.dispose();
    };
  }, [createSim, store, selfId]);
  useEffect(
    () => seating?.setStandingCheck((feet) => !sim || sim.standingAt(feet, store.getState().worldSettings.charRadius)),
    [seating, sim, store],
  );

  const { catalog, voxelSize } = useApplication();
  const kinds = useCatalog();
  const characters = useCatalogCharacters();
  const worldObjects = useStoreWorldObjects(store);
  const loadedBakedColliders = useBakedColliders(bakedLayouts, props.onError);
  const bakedColliders = useMemo(
    (): SimBakedColliders[] =>
      (bakedLayouts ?? []).flatMap((p) => {
        const colliders = loadedBakedColliders.get(p.roomInstanceId);
        if (colliders === null) {
          // A stale bake embeds none: its compiled store instances collide instead.
          return [];
        }
        const { position, rotation } = placementTransform(p);
        return [
          {
            roomInstanceId: p.roomInstanceId,
            position: { x: position[0], y: position[1], z: position[2] },
            yawRad: rotation[1],
            colliders: colliders ?? [],
          },
        ];
      }),
    [bakedLayouts, loadedBakedColliders],
  );
  const notifiedColliders = useRef(new Set<string>());
  const notifyCollidersReady = useEffectEvent((ids: readonly string[]) => {
    for (const id of ids) {
      if (loadedBakedColliders.has(id) && !notifiedColliders.current.has(id)) {
        notifiedColliders.current.add(id);
        props.onCollidersReady?.(id);
      }
    }
  });
  useEffect(() => {
    if (!sim) {
      return;
    }
    sim.setStaticColliders(worldObjects, { voxelSize, kinds: catalog.listKinds() }, bakedColliders);
    notifyCollidersReady(bakedColliders.map((b) => b.roomInstanceId));
  }, [sim, worldObjects, voxelSize, catalog, kinds, bakedColliders]);

  // E sits (walking, jumping and the stick stand up again through the frame). Space and the arrows
  // would scroll the page. Bevy hears the keys itself; this only adds what it doesn't do.
  const sitsOnE = props.cameraMode !== 'free-fly' ? seating : undefined;
  useEffect(() => {
    if (!props.worldFocused) {
      return;
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target) || hasCommandModifier(e)) {
        return;
      }
      if (SCROLL_KEYS.has(e.code)) {
        e.preventDefault();
      }
      if (e.code === 'KeyE' && !e.repeat) {
        sitsOnE?.sit();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [sitsOnE, props.worldFocused]);

  const charRadius = useStoreSelector(store, selectCharRadius);
  const { cameraMode, view: constants } = props;
  const invertLook = props.invertLook ?? false;
  useEffect(() => {
    bevy?.wasm.setCameraMode(bevy.view, cameraMode);
  }, [bevy, cameraMode]);
  useEffect(() => {
    bevy?.wasm.setCameraConfig(bevy.view, cameraConfig(constants, charRadius, invertLook));
  }, [bevy, constants, charRadius, invertLook]);

  const excludeInstance = useMemo(() => excludeLayoutInstances(bakedLayouts), [bakedLayouts]);
  const bakedLayoutDiffs = useMemo(() => (bakedLayouts ?? []).map(bakedLayoutDiff), [bakedLayouts]);
  const frame = useRef<RoomFrame | null>(null);
  const options = {
    sim,
    cameraMode,
    worldFocused: props.worldFocused,
    selfReady: props.selfReady,
    spawnPoints: props.spawnPoints ?? NO_SPAWNS,
    moveInput: props.moveInputRef,
    autoWalk: props.autoWalk,
    seating,
    characterOf: (p: PlayerState) => pickCharacterForPlayer(p.id, p.avatar.model, characters),
    kinds,
    bakedLayouts: bakedLayoutDiffs,
    excludeInstance,
  };
  // The frame reads the latest props each step; it keeps its own state across renders.
  useLayoutEffect(() => {
    if (frame.current) {
      frame.current.options = options;
    }
  });

  const onProgress = useEffectEvent((percent: number) => props.onLoadProgress?.(percent));
  const onModelsReady = useEffectEvent(() => props.onModelsReady?.());
  const onFrameStats = useEffectEvent((sample: FrameStatsSample | null) => props.onFrameStats?.(sample));
  const { actions, rules, bus, handshake, bots, viewYaw } = props;
  useEffect(() => {
    if (!bevy) {
      return;
    }
    const { wasm, view } = bevy;
    const room = new RoomFrame(
      {
        store,
        actions,
        bus,
        bots,
        selfId,
        view: roomView(wasm, view),
        onViewYaw: (yaw) => viewYaw?.set(yaw),
      },
      options,
    );
    frame.current = room;
    const loading = { done: false, mostPending: 0, percent: -1 };
    const stats = { frames: 0, elapsedMs: 0, lastMs: performance.now() };
    const loop = new FrameLoop({ actions, store, rules, bus, sync, handshake }, (dtSec, nowMs) => {
      room.step(dtSec, nowMs);
      if (!loading.done) {
        const { ready, pending } = wasm.isReady(view);
        loading.mostPending = Math.max(loading.mostPending, pending);
        const percent = ready ? 100 : Math.floor((100 * (loading.mostPending - pending)) / Math.max(loading.mostPending, 1));
        if (percent !== loading.percent) {
          loading.percent = percent;
          onProgress(percent);
        }
        if (ready) {
          loading.done = true;
          (window as BevySentinels).__officexrBevyReady = true;
          onModelsReady();
        }
      }
      // Real frame time, not the loop's capped dt.
      stats.frames += 1;
      stats.elapsedMs += nowMs - stats.lastMs;
      stats.lastMs = nowMs;
      if (stats.elapsedMs >= STATS_INTERVAL_MS) {
        const { drawCalls, triangles } = wasm.frameStats(view);
        onFrameStats({ fps: (1000 * stats.frames) / stats.elapsedMs, frameMs: stats.elapsedMs / stats.frames, drawCalls, triangles });
        stats.frames = 0;
        stats.elapsedMs = 0;
      }
    }, { onError: (error) => reportError(error) });
    loop.start();
    return () => {
      loop.stop();
      frame.current = null;
      onFrameStats(null);
    };
    // The frame takes every other input through `options`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bevy, store, actions, rules, bus, sync, handshake, bots, selfId, viewYaw]);

  return <div ref={container} style={{ position: 'absolute', inset: 0, touchAction: 'none' }} />;
}
