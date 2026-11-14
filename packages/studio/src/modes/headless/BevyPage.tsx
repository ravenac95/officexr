import React, { useEffect, useMemo, useState } from 'react';
import { createPersistentLocalState } from '@officexr/core-refactor';
import type { OfficeState } from '@officexr/sdk';
import { modelUrl } from '@officexr/world/app';
import { pickCharacterForPlayer } from '@officexr/world/characters';
import { useApplication, usePlaySession } from '@officexr/world/react';
import { createStorages } from '../debug/DebugApp.tsx';
import { useMapPicker } from '../debug/useMapPicker.ts';
import { DEFAULT_MUGSHOT_BACKGROUND, DEFAULT_MUGSHOT_LIGHTING, cubesForMode } from '../mugshot/MugshotApp.tsx';
import { buildInMemoryStack } from '../../realtime/services.ts';

type BevyModule = typeof import('@officexr/render-wasm');
type BakedLayout = ReturnType<typeof usePlaySession>['bakedLayouts'][number];

interface BevyCamera {
  position: [number, number, number];
  lookAt: [number, number, number];
  fovDeg: number;
  near: number;
  far: number;
}

interface BevySentinels {
  /** The wasm module, for drivers to call `setScene` on directly. */
  __officexrBevy?: BevyModule;
  __officexrBevyReady?: boolean;
}

const sentinels = () => window as unknown as BevySentinels;

const SELF_ID = 'bevy-player';
const CANVAS_ID = 'bevy-canvas';
/** The Mugshot mode's 512 px viewport inside its 1 px border, as its ideal PNGs are cropped. */
const MUGSHOT_SIZE_PX = 510;
const NEAR = 0.1;
const FAR = 2000;

let booted: Promise<BevyModule> | null = null;

/** Bevy owns its event loop for the page's lifetime, so it boots once. */
function bootBevy(): Promise<BevyModule> {
  booted ??= import('@officexr/render-wasm').then(async (wasm) => {
    await wasm.default();
    const origin = modelUrl('/models/').replace(/\/models\/$/, '') || window.location.origin;
    wasm.start(`#${CANVAS_ID}`, origin);
    return wasm;
  });
  return booted;
}

function useBevy(onBoot: (wasm: BevyModule) => (() => void) | void) {
  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | void;
    let poll = 0;
    void bootBevy().then((wasm) => {
      if (cancelled) {
        return;
      }
      sentinels().__officexrBevy = wasm;
      cleanup = onBoot(wasm);
      poll = window.setInterval(() => {
        sentinels().__officexrBevyReady = wasm.isReady();
      }, 100);
    });
    return () => {
      cancelled = true;
      window.clearInterval(poll);
      cleanup?.();
      delete sentinels().__officexrBevy;
      delete sentinels().__officexrBevyReady;
    };
    // Bevy boots once per page, so onBoot runs once.
  }, []);
}

/**
 * `?op=bevy`: the current map drawn by `crates/officexr-render` compiled to
 * wasm, fed from the SDK store. `&mugshot=<character>` draws the Mugshot
 * mode's scene instead. `window.__officexrBevyReady` turns true once every
 * model has loaded.
 */
export function BevyPage({ mugshot }: { mugshot: string | null }) {
  return (
    <div style={{ position: 'absolute', inset: 0, background: '#000' }}>
      <div
        style={
          mugshot
            ? { width: MUGSHOT_SIZE_PX, height: MUGSHOT_SIZE_PX }
            : { position: 'absolute', inset: 0 }
        }
      >
        <canvas id={CANVAS_ID} />
      </div>
      {mugshot ? <MugshotFeed character={mugshot} /> : <StoreFeed />}
    </div>
  );
}

/** As `bevy-mugshot-diff.spec.ts`: the Mugshot mode's fixed camera at azimuth 180°. */
function MugshotFeed({ character }: { character: string }) {
  const { catalog } = useApplication();
  useBevy((wasm) => {
    const instances = cubesForMode('gltf');
    const kindIds = new Set(instances.map((i) => i.kindId));
    const scene = {
      kinds: catalog.getCatalog().kinds.filter((k) => kindIds.has(k.id)),
      objects: { cubeSize: 0.5, instances },
      lighting: DEFAULT_MUGSHOT_LIGHTING,
      background: DEFAULT_MUGSHOT_BACKGROUND,
    };
    const pitch = (-8 * Math.PI) / 180;
    const distance = 6;
    const camera: BevyCamera = {
      position: [0, 1.7, distance],
      lookAt: [0, 1.7 + Math.sin(pitch), distance - Math.cos(pitch)],
      fovDeg: 40,
      near: NEAR,
      far: FAR,
    };
    wasm.setScene(JSON.stringify(scene), camera, character);
  });
  return null;
}

function StoreFeed() {
  const { catalog } = useApplication();
  const [local] = useState(() => createPersistentLocalState({ selfId: SELF_ID, officeId: 'bevy' }));
  const storages = useMemo(createStorages, []);
  const picker = useMapPicker(storages.maps);
  const session = usePlaySession({ actions: local.actions, mapName: picker.selected, storages });

  useEffect(() => {
    (window as unknown as { __OFFICE_STORE__: typeof local.store }).__OFFICE_STORE__ = local.store;
  }, [local]);

  const [wasm, setWasm] = useState<BevyModule | null>(null);
  useBevy(setWasm);

  const { loaded, bakedLayouts } = session;
  useEffect(() => {
    if (!wasm || !loaded) {
      return;
    }
    const state = local.store.getState();
    const usedKindIds = new Set(state.worldObjects.instances.map((i) => i.kindId));
    const scene = {
      kinds: catalog.getCatalog().kinds.filter((k) => usedKindIds.has(k.id)),
      objects: state.worldObjects,
      lighting: state.worldConstants.lighting,
      background: state.worldConstants.background,
      bakedLayouts: bakedLayouts.map(layoutPlacement),
    };
    wasm.setScene(JSON.stringify(scene), OVERVIEW_CAMERA, undefined);
  }, [wasm, loaded, bakedLayouts, local, catalog]);

  // One wandering in-browser bot keeps a player moving.
  useEffect(() => {
    if (!wasm || !loaded) {
      return;
    }
    const audio = new Audio();
    const built = buildInMemoryStack({ local, audio }).then(async (stack) => {
      stack.bots.setMode('wander');
      await stack.bots.setCount(1);
      return stack;
    });
    let frame = 0;
    let last = performance.now();
    const tick = (now: number) => {
      void built.then((stack) => stack.bots.tick((now - last) / 1000));
      last = now;
      wasm.setPlayers(playersInput(local.store.getState()));
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      void built.then((stack) => stack.teardown());
    };
  }, [wasm, loaded, local]);

  return null;
}

const layoutPlacement = (p: BakedLayout) => ({
  gltfPath: `${window.location.origin}/api/baked-layouts/${p.layoutName}.glb`,
  position: p.position,
  rotationY: p.rotationY,
});

// ponytail: a fixed camera over the origin, where the bot spawns; no follow camera or orbit controls.
const OVERVIEW_CAMERA: BevyCamera = { position: [0, 12, 16], lookAt: [0, 0, 0], fovDeg: 60, near: NEAR, far: FAR };

// ponytail: the local player has no sim here, so only the bot is drawn; peers aren't extrapolated.
function playersInput(state: OfficeState) {
  const ws = state.worldSettings;
  return {
    players: Object.values(state.players)
      .filter((p) => p.id !== SELF_ID)
      .map((p) => ({
        id: p.id,
        character: pickCharacterForPlayer(p.id, p.avatar.model),
        pos: [p.pos.x, p.pos.y, p.pos.z],
        yaw: p.yaw,
        vel: [p.vel.x, p.vel.y, p.vel.z],
        name: p.name,
        airborne: p.isAirborne,
      })),
    charRadius: ws.charRadius,
    playerSpeed: ws.playerSpeed,
    runSpeed: ws.playerSpeed * ws.runSpeedMultiplier,
  };
}
