import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPersistentLocalState } from '@officexr/core-refactor';
import type { CameraPose, PickEvent, RendererModule, ViewId } from '@officexr/render-wasm';
import type { OfficeState } from '@officexr/sdk';
import { pickCharacterForPlayer } from '@officexr/world/characters';
import { useApplication, usePlaySession } from '@officexr/world/react';
import { createStorages } from '../debug/DebugApp.tsx';
import { useMapPicker } from '../debug/useMapPicker.ts';
import {
  DEFAULT_MUGSHOT_BACKGROUND,
  DEFAULT_MUGSHOT_LIGHTING,
  mugshotCamera,
  mugshotWorld,
} from '../mugshot/mugshotScene.ts';
import { buildInMemoryStack } from '../../realtime/services.ts';
import { BevyCanvas, bevySentinels as sentinels, useReadySentinel } from './BevyCanvas.tsx';

type BakedLayout = ReturnType<typeof usePlaySession>['bakedLayouts'][number];

const SELF_ID = 'bevy-player';
/** The Mugshot mode's 512 px viewport inside its 1 px border, as its ideal PNGs are cropped. */
const MUGSHOT_SIZE_PX = 510;
/** The character the second view of `views=2` previews. */
const PREVIEW_CHARACTER = 'Barbarian';
const NEAR = 0.1;
const FAR = 2000;

/**
 * `?op=bevy`: the current map drawn by `crates/officexr-render` compiled to
 * wasm, fed from the SDK store. `&mugshot=<character>` draws the Mugshot
 * mode's scene instead; `&views=2` draws both, side by side, from one Bevy
 * app. `window.__officexrBevyReady` turns true once every model has loaded.
 * `&pick=1` logs the map view's pick events.
 */
export function BevyPage({ mugshot, views, pick }: { mugshot: string | null; views: number; pick: boolean }) {
  useReadySentinel();
  if (views === 2) {
    return (
      <div style={{ position: 'absolute', inset: 0, display: 'flex', background: '#000' }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <StoreFeed id="bevy-canvas" pick={pick} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <MugshotFeed id="bevy-canvas-2" character={PREVIEW_CHARACTER} />
        </div>
      </div>
    );
  }
  return (
    <div style={{ position: 'absolute', inset: 0, background: '#000' }}>
      <div
        style={
          mugshot
            ? { width: MUGSHOT_SIZE_PX, height: MUGSHOT_SIZE_PX }
            : { position: 'absolute', inset: 0 }
        }
      >
        {mugshot ? <MugshotFeed id="bevy-canvas" character={mugshot} primary /> : <StoreFeed id="bevy-canvas" pick={pick} />}
      </div>
    </div>
  );
}

/** The Mugshot mode's fixed camera at azimuth 180°. */
function MugshotFeed({ id, character, primary = false }: { id: string; character: string; primary?: boolean }) {
  const { catalog } = useApplication();
  return (
    <BevyCanvas
      id={id}
      kind="preview"
      primary={primary}
      onView={(wasm, view) => {
        let cancelled = false;
        // The character settles on the kinds' colliders, so it waits for the served catalog.
        void catalog.ready().then(() => {
          if (cancelled) {
            return;
          }
          const scene = mugshotWorld(catalog.getCatalog().kinds, 'gltf', DEFAULT_MUGSHOT_LIGHTING, DEFAULT_MUGSHOT_BACKGROUND);
          sentinels().__officexrBevyMugshotFeetY = wasm.setWorld(view, JSON.stringify(scene), character);
          wasm.setCamera(view, mugshotCamera({ azimuthDeg: 180, distanceM: 6, height: 1.7 }));
        });
        return () => {
          cancelled = true;
        };
      }}
    />
  );
}

/** Logs a pick event and keeps it in `window.__officexrBevyPicks`; `move`s would drown the rest. */
function logPick(event: PickEvent) {
  if (event.kind === 'move') {
    return;
  }
  console.debug('[bevy pick]', event.kind, event.target, event.point);
  (sentinels().__officexrBevyPicks ??= []).push(event);
}

function StoreFeed({ id, pick }: { id: string; pick: boolean }) {
  const { catalog } = useApplication();
  const [local] = useState(() => createPersistentLocalState({ selfId: SELF_ID, officeId: 'bevy' }));
  const storages = useMemo(createStorages, []);
  const picker = useMapPicker(storages.maps);
  const session = usePlaySession({ actions: local.actions, mapName: picker.selected, storages });

  useEffect(() => {
    (window as unknown as { __OFFICE_STORE__: typeof local.store }).__OFFICE_STORE__ = local.store;
  }, [local]);

  const [bevy, setBevy] = useState<{ wasm: RendererModule; view: ViewId } | null>(null);
  const sentIds = useRef(new Set<string>());

  const { ready, loaded, bakedLayouts } = session;
  // A map that fails to load still gets the sky, under its error, rather than a blank canvas.
  useEffect(() => {
    if (!bevy || !ready) {
      return;
    }
    const state = local.store.getState();
    const { instances } = state.worldObjects;
    const ids = new Set(instances.map((i) => i.id));
    const usedKindIds = new Set(instances.map((i) => i.kindId));
    const diff = {
      kinds: catalog.getCatalog().kinds.filter((k) => usedKindIds.has(k.id)),
      objects: { instances },
      remove: [...sentIds.current].filter((i) => !ids.has(i)),
      lighting: state.worldConstants.lighting,
      background: state.worldConstants.background,
      bakedLayouts: bakedLayouts.map(layoutPlacement),
    };
    sentIds.current = ids;
    bevy.wasm.setWorld(bevy.view, JSON.stringify(diff));
    bevy.wasm.setCamera(bevy.view, OVERVIEW_CAMERA);
  }, [bevy, ready, bakedLayouts, local, catalog]);

  // One wandering in-browser bot keeps a player moving.
  useEffect(() => {
    if (!bevy || !loaded) {
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
      bevy.wasm.setPlayers(bevy.view, playersInput(local.store.getState()));
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      void built.then((stack) => stack.teardown());
    };
  }, [bevy, loaded, local]);

  return (
    <>
      <BevyCanvas
        id={id}
        kind="world"
        primary
        onView={(wasm, view) => {
          setBevy({ wasm, view });
          if (pick) {
            wasm.onPick(view, logPick);
          }
        }}
      />
      {session.error && (
        <p role="alert" style={{ position: 'absolute', top: 8, left: 8, margin: 0, color: '#fff', font: '13px sans-serif' }}>
          {session.error}
        </p>
      )}
    </>
  );
}

const layoutPlacement = (p: BakedLayout) => ({
  gltfPath: `${window.location.origin}/api/baked-layouts/${p.layoutName}.glb`,
  position: p.position,
  rotationY: p.rotationY,
  layoutName: p.layoutName,
  roomInstanceId: p.roomInstanceId,
});

// ponytail: a fixed camera over the origin, where the bot spawns; no follow camera or orbit controls.
const OVERVIEW_CAMERA: CameraPose = { position: [0, 12, 16], lookAt: [0, 0, 0], fovDeg: 60, near: NEAR, far: FAR };

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
