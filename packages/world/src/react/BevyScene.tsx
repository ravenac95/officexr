import React, { useEffect, useEffectEvent, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { SoundFrame } from '../audio/soundFrame.ts';
import { useSfx } from '../audio/sfx-context.tsx';
import type { CameraConfig, JoystickKind, RendererModule, ViewId } from '@officexr/render-wasm';
import type {
  Actions,
  Bus,
  PlayerState,
  RuleRegistry,
  SnapshotHandshake,
  Store,
  SyncEngine,
  Vec3,
  WorldConstants,
  WorldObjects,
} from '@officexr/sdk';
import type { BotPool } from '../bot/BotPool.ts';
import { resolveCharacter } from '../characters/registry.ts';
import type { CameraMode } from '../renderer/config.ts';
import { isYawPitchMode } from '../renderer/lookControls.ts';
import { excludeRacks, occupiedRacks } from '../renderer/occupiedRacks.ts';
import { useMotionWorld } from '../renderer/useMotionWorld.ts';
import { heldStands, swapHeldStands } from '../motion/wand.ts';
import type { ActionInput } from '../input/action-input.ts';
import type { InteractionStore } from '../input/interaction.ts';
import type { PlacementHost } from '../motion/place-mode.ts';
import type { MissionMarkerTarget } from '../renderer/missionMarker.ts';
import type { FixedCameraViewConfig } from '../renderer/viewConfig.ts';
import type { FrameStatsSample } from './frameStatsMeter.ts';
import type { MotionLookState } from '../renderer/motionLook.ts';
import type { AutoWalkSource, MoveInput } from '../renderer/moveIntent.ts';
import { placementTransform } from '../renderer/placement-transform.ts';
import type { SeatingSource } from '../renderer/seated.ts';
import { useBakedColliders, type SceneBakedLayout } from '../renderer/useBakedColliders.ts';
import { useStoreWorldObjects } from '../renderer/useStoreWorldObjects.ts';
import { excludeLayoutInstances } from '../scenes/compile-map.ts';
import type { SimBakedColliders, SimCore } from '../sim/SimCore.ts';
import { useApplication, useCatalog, useCatalogCharacters } from './application-context.tsx';
import { bakedLayoutGlbUrl } from './bevyRenderer.ts';
import { FrameLoop } from './frameLoop.ts';
import { PlacementFrame } from './placementFrame.ts';
import { PlayInput } from './playInput.ts';
import { pingBellPicks } from './pingBell.ts';
import { frameStatsMeter } from './frameStatsMeter.ts';
import { PlayerOverheads, type PlayerOverheadsHandle } from './PlayerOverheads.tsx';
import { frozenLoginOf, RoomFrame, type RoomView } from './roomFrame.ts';
import { useFrozenAvatars } from './useFrozenAvatars.ts';
import { useWaterVolumes } from '../renderer/useWaterVolumes.ts';
import { useCreateSim, type SimImplementation } from './useCreateSim.ts';
import { setBevyReady, usePrimaryBevyView } from './usePrimaryBevyView.ts';
import { useStoreSelector } from './useStoreSelector.ts';
import { hasCommandModifier, isTypingTarget } from './useWorldFocus.ts';
import type { ExcludeInstance } from '../scenes/compile-map.ts';
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
  view: WorldConstants & { fixedCamera: FixedCameraViewConfig };
  /** The view cannot be turned by dragging. */
  lookLocked?: boolean;
  /** Discrete actions (jump, sprint, interact, poke, emote) from keys and the touch action pad. */
  actionInput?: ActionInput;
  /** What the player can interact with nearby, for the interaction prompt. */
  interactions?: InteractionStore;
  /** Shows Ping on a frozen avatar's card. */
  onPingFrozen?: (login: string) => void;
  /** Lets the player place props; absent where nobody builds. */
  placement?: PlacementHost;
  missionMarker?: MissionMarkerTarget | null;
  worldFocused: boolean;
  moveInputRef?: React.RefObject<MoveInput>;
  /** Which touch joystick walks: Bevy's own, or the DOM one in `moveInputRef`. */
  joystickKind: JoystickKind;
  onDomJoystick?: (dom: boolean) => void;
  motionLook?: MotionLookState;
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
  /** A click on the bell over a hovered player. */
  onPing?: (view: ViewId, playerId: string) => void;
}

const SCROLL_KEYS = new Set(['Space', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']);
const NO_SPAWNS: readonly Vec3[] = [];

const selectCharRadius = (s: { worldSettings: { charRadius: number } }) => s.worldSettings.charRadius;
const selectRacksInUse = (s: { players: Parameters<typeof occupiedRacks>[0] }) => occupiedRacks(s.players);
const selectHeldStands = (s: { players: Parameters<typeof heldStands>[0] }) => heldStands(s.players);

// ponytail: no bake version in the URL, so Bevy keeps drawing a layout re-baked mid-session until a reload.
function bakedLayoutDiff(p: SceneBakedLayout): BakedLayoutDiff {
  return { gltfPath: bakedLayoutGlbUrl(p.gltfPath), position: p.position, rotationY: p.rotationY };
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

/** `worldObjects` less the instances `exclude` drops; the same object when it drops none. */
function without(worldObjects: WorldObjects, exclude: ExcludeInstance | undefined): WorldObjects {
  if (!exclude) {
    return worldObjects;
  }
  const instances = worldObjects.instances.filter((i) => !exclude(i));
  return instances.length === worldObjects.instances.length ? worldObjects : { ...worldObjects, instances };
}

function roomView(wasm: RendererModule, view: ViewId): RoomView {
  return {
    takeIntent: () => wasm.takeIntent(view),
    setInputEnabled: (enabled) => wasm.setInputEnabled(view, enabled),
    setStick: (stick) => wasm.setStick(view, stick),
    setJoystickKind: (kind) => wasm.setJoystickKind(view, kind),
    setWorld: (diff) => void wasm.setWorld(view, JSON.stringify(diff)),
    setPlayers: (players) => wasm.setPlayers(view, players),
    setFollowTarget: (pos, yaw) => wasm.setFollowTarget(view, pos, yaw),
    setConversationFocus: (point) => wasm.setConversationFocus(view, point),
    setProximity: (proximity) => wasm.setProximity(view, proximity),
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
  const reportError = useEffectEvent((error: Error) => props.onError?.(error));
  const bevy = usePrimaryBevyView(container, (error) => props.onError?.(error));

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

  const { catalog, voxelSize, geometry } = useApplication();
  const kinds = useCatalog();
  const characters = useCatalogCharacters();
  // A ridden rack's vehicle goes with its rider: the rack is neither drawn nor solid.
  const racksInUse = useStoreSelector(store, selectRacksInUse);
  const ridden = useMemo(() => excludeRacks(racksInUse, undefined), [racksInUse]);
  const storeWorldObjects = useStoreWorldObjects(store);
  const worldObjects = useMemo(() => without(storeWorldObjects, ridden), [storeWorldObjects, ridden]);
  const motionWorld = useMotionWorld(store);
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

  // Space and the arrows would scroll the page. Bevy hears the movement keys itself, and the frame
  // reads the gameplay keys (E, F, Q, the digits) from `playInput`.
  useEffect(() => {
    if (!props.worldFocused) {
      return;
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (!isTypingTarget(e.target) && !hasCommandModifier(e) && SCROLL_KEYS.has(e.code)) {
        e.preventDefault();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [props.worldFocused]);
  const [playInput, setPlayInput] = useState<PlayInput | null>(null);
  useEffect(() => {
    const surface = container.current;
    if (!surface) {
      return;
    }
    const created = new PlayInput(window, surface);
    setPlayInput(created);
    return () => {
      setPlayInput(null);
      created.dispose();
    };
  }, []);
  useEffect(() => playInput?.setEnabled(props.worldFocused), [playInput, props.worldFocused]);

  const charRadius = useStoreSelector(store, selectCharRadius);
  const { cameraMode, view: constants } = props;
  const invertLook = props.invertLook ?? false;
  useEffect(() => {
    bevy?.wasm.setCameraMode(bevy.view, cameraMode);
  }, [bevy, cameraMode]);
  useEffect(() => {
    bevy?.wasm.setCameraConfig(bevy.view, cameraConfig(constants, charRadius, invertLook));
  }, [bevy, constants, charRadius, invertLook]);
  // ponytail: the Bevy stick stays anchored bottom-left while the look is locked; the DOM one floats.
  const lookLocked = props.lookLocked ?? false;
  useEffect(() => {
    bevy?.wasm.setLookLocked(bevy.view, lookLocked);
  }, [bevy, lookLocked]);

  // ponytail: no recenter; `motionLook.recenterKey` is ignored on the Bevy view.
  // A reading with nulls would reset the view's previous pose, so only the cleanup sends one.
  const motionLook = props.motionLook?.enabled ?? false;
  useEffect(() => {
    if (!bevy || !motionLook) {
      return;
    }
    const { wasm, view } = bevy;
    const onOrientation = ({ alpha, beta, gamma }: DeviceOrientationEvent) => {
      if (alpha !== null && beta !== null && gamma !== null) {
        wasm.setDeviceOrientation(view, { alpha, beta, gamma });
      }
    };
    window.addEventListener('deviceorientation', onOrientation);
    return () => {
      window.removeEventListener('deviceorientation', onOrientation);
      wasm.setDeviceOrientation(view, { alpha: null, beta: null, gamma: null });
    };
  }, [bevy, motionLook]);

  const held = useStoreSelector(store, selectHeldStands);
  const drawnObjects = useMemo(() => {
    const stands = new Set(held ? held.split('|') : []);
    return (w: WorldObjects) => swapHeldStands(w, catalog, stands);
    // `kinds` changes when the catalog's kinds do; `catalog` keeps its identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [held, catalog, kinds]);
  const excludeInstance = useMemo(
    () => excludeRacks(racksInUse, excludeLayoutInstances(bakedLayouts)),
    [racksInUse, bakedLayouts],
  );
  const frozen = useFrozenAvatars(store);
  const waterVolumes = useWaterVolumes(store);
  const overheads = useRef<PlayerOverheadsHandle>(null);
  const bakedLayoutDiffs = useMemo(() => (bakedLayouts ?? []).map(bakedLayoutDiff), [bakedLayouts]);
  const frame = useRef<RoomFrame | null>(null);
  const options = {
    sim,
    cameraMode,
    worldFocused: props.worldFocused,
    selfReady: props.selfReady,
    spawnPoints: props.spawnPoints ?? NO_SPAWNS,
    moveInput: props.moveInputRef,
    joystickKind: props.joystickKind,
    autoWalk: props.autoWalk,
    seating,
    actionInput: props.actionInput,
    interactions: props.interactions,
    motionWorld,
    // The model's URL, not its id: a catalog character's id names no `/models/characters/` file.
    characterOf: (p: PlayerState) => resolveCharacter(p.id, p.avatar.model, characters).url,
    kinds,
    bakedLayouts: bakedLayoutDiffs,
    excludeInstance,
    drawnObjects,
    frozenAvatars: () => frozen,
    waterVolumes,
    hostTags: true,
  };
  // The frame reads the latest props each step; it keeps its own state across renders.
  useLayoutEffect(() => {
    if (frame.current) {
      frame.current.options = options;
    }
  });

  const onProgress = useEffectEvent((percent: number) => props.onLoadProgress?.(percent));
  const onModelsReady = useEffectEvent(() => props.onModelsReady?.());
  const onPing = useEffectEvent((view: ViewId, playerId: string) => props.onPing?.(view, playerId));
  const onDomJoystick = useEffectEvent((dom: boolean) => props.onDomJoystick?.(dom));
  const onFrameStats = useEffectEvent((sample: FrameStatsSample | null) => props.onFrameStats?.(sample));
  const { actions, rules, bus, handshake, bots, viewYaw } = props;
  const sfx = useSfx();
  const sounds = useRef<SoundFrame | null>(null);
  useEffect(() => {
    if (!sfx) {
      return;
    }
    const created = new SoundFrame(() => store.getState(), bus, sfx);
    sounds.current = created;
    return () => {
      sounds.current = null;
      created.dispose();
    };
  }, [sfx, store, bus]);

  const { placement } = props;
  const building = useRef<PlacementFrame | null>(null);
  useEffect(() => {
    if (!placement) {
      return;
    }
    // ponytail: nothing draws the ghost yet; the view will take `placementPreview(placement.mode.get(), …)`.
    const created = new PlacementFrame({
      host: placement,
      getState: () => store.getState(),
      geometry,
      target: window,
      pointerLocked: () => document.pointerLockElement !== null,
    });
    building.current = created;
    return () => {
      building.current = null;
      created.dispose();
    };
  }, [placement, store, geometry]);

  useEffect(() => {
    if (!bevy || !playInput) {
      return;
    }
    const { wasm, view } = bevy;
    const bellPicks = pingBellPicks(view, onPing);
    wasm.onPick(
      view,
      (event) => {
        bellPicks(event);
        // A click on an away claimant's avatar opens their card, as `FrozenAvatars.tsx`'s hit capsule did.
        const login = event.kind === 'click' && event.target.type === 'player' ? frozenLoginOf(event.target.id) : null;
        if (login !== null) {
          overheads.current?.toggleFrozen(login);
        }
      },
      { bellClaimsOnly: true },
    );
    const room = new RoomFrame(
      {
        store,
        actions,
        bus,
        sync,
        bots,
        selfId,
        view: roomView(wasm, view),
        keys: playInput,
        onViewYaw: (yaw) => viewYaw?.set(yaw),
        onDomJoystick: (dom) => onDomJoystick(dom),
      },
      options,
    );
    frame.current = room;
    const loading = { done: false, mostPending: 0, percent: -1 };
    const meterStats = frameStatsMeter(() => wasm.frameStats(view), onFrameStats);
    const loop = new FrameLoop({ actions, store, rules, bus, sync, handshake }, (dtSec, nowMs) => {
      room.step(dtSec, nowMs);
      building.current?.step();
      sounds.current?.step(nowMs);
      overheads.current?.place(wasm.playerAnchors(view));
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
          setBevyReady(true);
          onModelsReady();
        }
      }
      meterStats(nowMs);
    }, { onError: (error) => reportError(error) });
    loop.start();
    return () => {
      loop.stop();
      frame.current = null;
      room.dispose();
      wasm.onPick(view, null);
      onFrameStats(null);
    };
    // The frame takes every other input through `options`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bevy, playInput, store, actions, rules, bus, sync, handshake, bots, selfId, viewYaw]);

  // The right button turns the view in first and third person, so it opens no menu there.
  const onContextMenu = isYawPitchMode(cameraMode) ? (e: React.MouseEvent) => e.preventDefault() : undefined;
  return (
    <>
      <div ref={container} onContextMenu={onContextMenu} style={{ position: 'absolute', inset: 0, touchAction: 'none' }} />
      <PlayerOverheads ref={overheads} store={store} bus={bus} selfId={selfId} frozen={frozen} onPingFrozen={props.onPingFrozen} />
    </>
  );
}
