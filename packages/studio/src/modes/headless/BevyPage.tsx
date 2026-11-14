import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPersistentLocalState } from '@officexr/core-refactor';
import { loadRenderer, type CameraPose, type RendererModule, type ViewId, type ViewKind } from '@officexr/render-wasm';
import type { OfficeState } from '@officexr/sdk';
import { modelUrl } from '@officexr/world/app';
import { pickCharacterForPlayer } from '@officexr/world/characters';
import { useApplication, usePlaySession } from '@officexr/world/react';
import { createStorages } from '../debug/DebugApp.tsx';
import { useMapPicker } from '../debug/useMapPicker.ts';
import { DEFAULT_MUGSHOT_BACKGROUND, DEFAULT_MUGSHOT_LIGHTING, cubesForMode } from '../mugshot/MugshotApp.tsx';
import { buildInMemoryStack } from '../../realtime/services.ts';

type BakedLayout = ReturnType<typeof usePlaySession>['bakedLayouts'][number];

interface BevySentinels {
  /** The wasm module, for drivers to call it directly. */
  __officexrBevy?: RendererModule;
  /** The page's view ids, in canvas order. */
  __officexrBevyViews?: ViewId[];
  /** Every view has loaded and settled. */
  __officexrBevyReady?: boolean;
  /** Where the mugshot character's feet settled, in metres. */
  __officexrBevyMugshotFeetY?: number;
}

const sentinels = () => window as unknown as BevySentinels;

const SELF_ID = 'bevy-player';
/** The Mugshot mode's 512 px viewport inside its 1 px border, as its ideal PNGs are cropped. */
const MUGSHOT_SIZE_PX = 510;
/** The character the second view of `views=2` previews. */
const PREVIEW_CHARACTER = 'Barbarian';
const NEAR = 0.1;
const FAR = 2000;

let booted: Promise<RendererModule> | null = null;

/**
 * Bevy owns its event loop for the page's lifetime, so it boots once, with the first caller's
 * `primary` canvas, if any, as its primary window.
 */
function bootBevy(primary?: HTMLCanvasElement): Promise<RendererModule> {
  booted ??= loadRenderer().then((wasm) => {
    const origin = modelUrl('/models/').replace(/\/models\/$/, '') || window.location.origin;
    wasm.init(origin, primary);
    return wasm;
  });
  return booted;
}

const canvasStyle: React.CSSProperties = { display: 'block', width: '100%', height: '100%' };

/** A Bevy view on a canvas for the component's lifetime; `onView` feeds it. */
function BevyCanvas({
  id,
  kind,
  primary = false,
  onView,
}: {
  id: string;
  kind: ViewKind;
  /** Draws into Bevy's primary window; mount it before the page's other canvases. */
  primary?: boolean;
  onView: (wasm: RendererModule, view: ViewId) => (() => void) | void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | void;
    let view: ViewId | null = null;
    void bootBevy(primary && canvas.current ? canvas.current : undefined).then((wasm) => {
      if (cancelled || !canvas.current) {
        return;
      }
      view = wasm.createView(canvas.current, kind);
      const s = sentinels();
      s.__officexrBevy = wasm;
      s.__officexrBevyViews = [...(s.__officexrBevyViews ?? []), view];
      cleanup = onView(wasm, view);
    });
    return () => {
      cancelled = true;
      cleanup?.();
      if (view !== null) {
        const gone = view;
        void bootBevy().then((wasm) => wasm.destroyView(gone));
        sentinels().__officexrBevyViews = sentinels().__officexrBevyViews?.filter((v) => v !== gone);
      }
    };
    // A view lives as long as its canvas, so onView runs once.
  }, []);
  return <canvas id={id} ref={canvas} style={canvasStyle} />;
}

/** Keeps `window.__officexrBevyReady` in step with every view's readiness. */
function useReadySentinel() {
  useEffect(() => {
    const poll = window.setInterval(() => {
      const s = sentinels();
      const views = s.__officexrBevyViews ?? [];
      s.__officexrBevyReady = views.length > 0 && views.every((v) => s.__officexrBevy?.isReady(v).ready);
    }, 100);
    return () => {
      window.clearInterval(poll);
      delete sentinels().__officexrBevy;
      delete sentinels().__officexrBevyViews;
      delete sentinels().__officexrBevyReady;
      delete sentinels().__officexrBevyMugshotFeetY;
    };
  }, []);
}

/**
 * `?op=bevy`: the current map drawn by `crates/officexr-render` compiled to
 * wasm, fed from the SDK store. `&mugshot=<character>` draws the Mugshot
 * mode's scene instead; `&views=2` draws both, side by side, from one Bevy
 * app. `window.__officexrBevyReady` turns true once every model has loaded.
 */
export function BevyPage({ mugshot, views }: { mugshot: string | null; views: number }) {
  useReadySentinel();
  if (views === 2) {
    return (
      <div style={{ position: 'absolute', inset: 0, display: 'flex', background: '#000' }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <StoreFeed id="bevy-canvas" />
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
        {mugshot ? <MugshotFeed id="bevy-canvas" character={mugshot} primary /> : <StoreFeed id="bevy-canvas" />}
      </div>
    </div>
  );
}

/** As `bevy-mugshot-diff.spec.ts`: the Mugshot mode's fixed camera at azimuth 180°. */
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
          sentinels().__officexrBevyMugshotFeetY = wasm.setWorld(view, JSON.stringify(scene), character);
          wasm.setCamera(view, {
            position: [0, 1.7, distance],
            lookAt: [0, 1.7 + Math.sin(pitch), distance - Math.cos(pitch)],
            fovDeg: 40,
            near: NEAR,
            far: FAR,
          });
        });
        return () => {
          cancelled = true;
        };
      }}
    />
  );
}

function StoreFeed({ id }: { id: string }) {
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
      <BevyCanvas id={id} kind="world" primary onView={(wasm, view) => setBevy({ wasm, view })} />
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
