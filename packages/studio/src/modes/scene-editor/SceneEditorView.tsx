import React, { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import type { PickEvent } from '@officexr/render-wasm';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import {
  bakedLayoutGlbUrl,
  blobAssetPath,
  diffWorld,
  frameStatsMeter,
  setBevyReady,
  useApplication,
  useCatalog,
  usePrimaryBevyView,
  type BevyView,
  type SentWorld,
} from '@officexr/world/react';
import { DEFAULT_EDITOR_LIGHTING } from '@officexr/world/renderer';
import { studioFetch } from '../../apiOrigin.ts';
import { publishFrameStats } from '../../perf/frame-stats.ts';
import type { SceneEditorBackend } from './SceneEditorBackend.ts';
import { editorOverlays } from './editorOverlays.ts';
import { editorPick, pointerProjector, roomCenter, type ScreenRay } from './editorPicks.ts';
import { useSceneEditorTools } from './useSceneEditorTools.ts';

type Vec3 = [number, number, number];

const LIGHTING = { ...DEFAULT_EDITOR_LIGHTING, ambientFillIntensity: 0.6 };
const CLEAR_COLOR = '#0a0a0a';
/** A press that moves further than this (CSS px) is a camera drag, not a click. */
const CLICK_SLOP_PX = 4;

function screenRay({ wasm, view }: BevyView, [x, y]: readonly [number, number]): ScreenRay {
  return {
    origin: () => wasm.rayAtScreen(view, x, y).origin,
    intersect: (plane) => wasm.intersectPlane(view, x, y, plane),
  };
}

/**
 * The baked layout GLB as a `blob:` URL Bevy can load: the studio API wants its session cookie or
 * preview token, which Bevy's own fetch doesn't send.
 */
function useBakedLayoutBlob(path: string | undefined): string | null {
  const [blob, setBlob] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let url: string | null = null;
    let cancelled = false;
    studioFetch(bakedLayoutGlbUrl(path))
      .then((r) => (r.ok ? r.blob() : Promise.reject(new Error(`baked layout: HTTP ${r.status}`))))
      .then((b) => {
        if (cancelled) return;
        url = URL.createObjectURL(b);
        setBlob(url);
      })
      .catch((e: unknown) => console.warn(e));
    return () => {
      cancelled = true;
      setBlob(null);
      if (url) URL.revokeObjectURL(url);
    };
  }, [path]);
  return blob;
}

/**
 * The room and layout editors' 3D view: a Bevy world view on the page's primary canvas, with the
 * controls' orbit camera. Its pointer input drives `useSceneEditorTools`; the document and the tools'
 * previews go back to the view as its world and overlays.
 */
export function SceneEditorView({ backend }: { backend: SceneEditorBackend }) {
  const tools = useSceneEditorTools(backend);
  const { geometry } = useApplication();
  const kinds = useCatalog();
  const container = useRef<HTMLDivElement>(null);
  const cameraHud = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const bevy = usePrimaryBevyView(container, (e) => setError(e.message));
  const { compiled, selection, tool, buildHeight } = backend;
  const voxelSize = compiled.cubeSize;

  useEffect(() => {
    bevy?.wasm.setCameraMode(bevy.view, 'orbit');
  }, [bevy]);

  useEffect(() => {
    if (!bevy) return;
    const meter = frameStatsMeter(() => bevy.wasm.frameStats(bevy.view), publishFrameStats);
    let frame = 0;
    const tick = (nowMs: number) => {
      meter(nowMs);
      setBevyReady(bevy.wasm.isReady(bevy.view).ready);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      publishFrameStats(null);
    };
  }, [bevy]);

  const layoutBlob = useBakedLayoutBlob(backend.bakedLayoutPath);
  const { bakedLayoutName } = backend;
  const bakedLayouts = useMemo(
    () =>
      layoutBlob
        ? [
            {
              gltfPath: blobAssetPath(layoutBlob, 'glb'),
              position: [0, 0, 0] as Vec3,
              rotationY: 0,
              layoutName: bakedLayoutName,
              roomInstanceId: '',
            },
          ]
        : [],
    [layoutBlob, bakedLayoutName],
  );
  const sentWorld = useRef<SentWorld | null>(null);
  useEffect(() => {
    if (!bevy) return;
    const { diff, sent } = diffWorld(sentWorld.current, {
      instances: compiled.instances,
      kinds,
      lighting: LIGHTING,
      background: DEFAULT_WORLD_CONSTANTS.background,
      bakedLayouts,
    });
    if (!diff) return;
    if (!sentWorld.current) diff.clearColor = CLEAR_COLOR;
    bevy.wasm.setWorld(bevy.view, JSON.stringify(diff));
    sentWorld.current = sent;
  }, [bevy, compiled.instances, kinds, bakedLayouts]);

  const [cx, cy, cz] = roomCenter(compiled.instances, voxelSize);
  useEffect(() => {
    bevy?.wasm.setOrbit(bevy.view, { target: [cx, cy, cz] });
  }, [bevy, cx, cy, cz]);

  const selectionBoxes = useMemo(
    () =>
      compiled.instances
        .filter((i) => selection.has(i.sourceCommandId))
        .map((i) => {
          const { min, max } = geometry.worldAABB(i.position, i.kindId, i.yawDeg);
          return { min: [...min] as Vec3, max: [...max] as Vec3 };
        }),
    [compiled.instances, selection, geometry],
  );
  const buildPlaneY = tool === 'add' || tool === 'tile' ? buildHeight * voxelSize : null;
  const { ghosts, gizmo } = tools;
  const overlays = useMemo(
    () => editorOverlays({ ghosts, selection: selectionBoxes, buildPlaneY, gizmo }),
    [ghosts, selectionBoxes, buildPlaneY, gizmo],
  );
  useEffect(() => {
    bevy?.wasm.setOverlays(bevy.view, overlays);
  }, [bevy, overlays]);

  const instancesById = useMemo(() => new Map(compiled.instances.map((i) => [i.id, i])), [compiled.instances]);
  // Bevy fires `click` after a drag on the same target, so a press that dragged is not a click.
  const pickDragged = useRef(false);
  // A Y-extrusion commit comes from the DOM release, which also sees releases over the sky; the
  // pick `click` for the same press, a frame later, must not commit again.
  const domCommitted = useRef(false);
  const domPress = useRef<[number, number] | null>(null);

  const handlePick = useEffectEvent((e: PickEvent) => {
    if (!bevy) return;
    const ray = screenRay(bevy, e.screen);
    const pick = editorPick(e, ray, { instancesById, footprintOf: tools.footprintOf, buildPlaneY: buildHeight * voxelSize });
    switch (e.kind) {
      case 'down':
        pickDragged.current = false;
        if (e.button === 0) tools.press(pick, pointerProjector(ray, voxelSize));
        return;
      case 'dragStart':
        pickDragged.current = true;
        return;
      case 'drag':
        if (e.button === 0) tools.drag(e.modifiers.shift, pointerProjector(ray, voxelSize));
        return;
      case 'move': {
        const camera = ray.origin();
        if (cameraHud.current) {
          cameraHud.current.textContent = `camera  x:${camera[0].toFixed(2)}  y:${camera[1].toFixed(2)}  z:${camera[2].toFixed(2)}`;
        }
        tools.hover(pick);
        return;
      }
      case 'out':
        tools.leave();
        return;
      case 'click': {
        if (pickDragged.current || domCommitted.current || tools.yAxis) return;
        if (e.button === 2) {
          const rect = container.current?.getBoundingClientRect();
          const commandId = pick.target?.type === 'object' ? pick.target.commandId : null;
          backend.onContextMenuRequest(commandId, (rect?.left ?? 0) + e.screen[0], (rect?.top ?? 0) + e.screen[1]);
        } else if (e.button === 0) {
          tools.click(pick);
        }
        return;
      }
    }
  });
  useEffect(() => {
    // Destroying the view drops its callback.
    bevy?.wasm.onPick(bevy.view, (e) => handlePick(e));
  }, [bevy]);

  const domRay = (e: React.PointerEvent): ScreenRay | null => {
    const rect = container.current?.getBoundingClientRect();
    return bevy && rect ? screenRay(bevy, [e.clientX - rect.left, e.clientY - rect.top]) : null;
  };

  // The Move drag ends at the real release, wherever it happens, as a window listener sees it.
  const onWindowPointer = useEffectEvent((e: PointerEvent) => {
    if (e.type === 'pointercancel') tools.cancelDrag();
    else if (e.button === 0) tools.release();
  });
  useEffect(() => {
    const listener = (e: PointerEvent) => onWindowPointer(e);
    window.addEventListener('pointerup', listener);
    window.addEventListener('pointercancel', listener);
    return () => {
      window.removeEventListener('pointerup', listener);
      window.removeEventListener('pointercancel', listener);
    };
  }, []);

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      <div
        ref={container}
        style={{ position: 'absolute', inset: 0, touchAction: 'none' }}
        onContextMenu={(e) => e.preventDefault()}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          domPress.current = [e.clientX, e.clientY];
          domCommitted.current = false;
        }}
        // Picks only fire over a target; the Y extrusion follows the pointer over the sky too.
        onPointerMove={(e) => {
          const ray = tools.yAxis && domRay(e);
          if (ray) tools.trackY(pointerProjector(ray, voxelSize));
        }}
        onPointerUp={(e) => {
          const press = domPress.current;
          domPress.current = null;
          if (e.button !== 0 || !press || !tools.yAxis) return;
          if (Math.max(Math.abs(e.clientX - press[0]), Math.abs(e.clientY - press[1])) > CLICK_SLOP_PX) return;
          const ray = domRay(e);
          if (!ray) return;
          domCommitted.current = true;
          tools.click({ target: null, hit: null, cameraY: ray.origin()[1], modKey: false, shift: false });
        }}
      />
      {/* What the snap's stack-above-or-below tie reads as the camera's height. */}
      <div ref={cameraHud} style={hudStyle}>
        camera  x:0.00  y:0.00  z:0.00
      </div>
      {tools.showAxisHint && <div style={axisHintStyle}>C — cycle axis</div>}
      {error && (
        <p role="alert" style={{ position: 'absolute', top: 8, left: 8, margin: 0, color: '#fff', font: '13px sans-serif' }}>
          {error}
        </p>
      )}
    </div>
  );
}

const hudStyle: React.CSSProperties = {
  position: 'absolute',
  right: 8,
  bottom: 8,
  padding: '4px 8px',
  background: 'rgba(0, 0, 0, 0.55)',
  color: '#fde68a',
  fontFamily: 'ui-monospace, monospace',
  fontSize: 11,
  lineHeight: 1.3,
  borderRadius: 4,
  pointerEvents: 'none',
  userSelect: 'none',
  zIndex: 5,
  whiteSpace: 'pre',
};

const axisHintStyle: React.CSSProperties = {
  position: 'absolute',
  bottom: 12,
  left: '50%',
  transform: 'translateX(-50%)',
  background: 'rgba(0,0,0,0.55)',
  color: '#e0e0e0',
  fontSize: 12,
  padding: '4px 10px',
  borderRadius: 4,
  pointerEvents: 'none',
  userSelect: 'none',
  fontFamily: 'monospace',
};
