import React, { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import type { PickEvent, RendererModule, ViewId } from '@officexr/render-wasm';
import { DEFAULT_WORLD_CONSTANTS, type ObjectInstance } from '@officexr/sdk';
import {
  bootBevy,
  diffWorld,
  frameStatsMeter,
  playCanvas,
  useApplication,
  useCatalog,
  type SentWorld,
} from '@officexr/world/react';
import { DEFAULT_EDITOR_LIGHTING } from '@officexr/world/renderer';
import { publishFrameStats } from '../../perf/frame-stats.ts';
import type { SnapHit } from '../room/roomSnap.ts';
import type { SceneEditorBackend } from './SceneEditorBackend.ts';
import { editorOverlays } from './editorOverlays.ts';
import { useSceneEditorTools, type PointerProjector } from './useSceneEditorTools.ts';

type Vec3 = [number, number, number];
type Bevy = { wasm: RendererModule; view: ViewId };

interface BevySentinels {
  /** The editor view has loaded every model; for Playwright. */
  __officexrBevyReady?: boolean;
}

const LIGHTING = { ...DEFAULT_EDITOR_LIGHTING, ambientFillIntensity: 0.6 };
const CLEAR_COLOR = '#0a0a0a';
/** A press that moves further than this (CSS px) is a camera drag, not a click. */
const CLICK_SLOP_PX = 4;
/** An up-facing hit (cos ≈ 45°) is a surface to rest on at its exact height. */
const UP_FACING_MIN_Y = 0.7;

/** Where the orbit camera looks: the middle of the room's instances. */
function roomCenter(instances: readonly ObjectInstance[], voxelSize: number): Vec3 {
  if (instances.length === 0) return [0, 1, 0];
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const { position } of instances) {
    const p = [position[0], position[1] + 0.5, position[2]];
    for (let i = 0; i < 3; i++) {
      min[i] = Math.min(min[i], p[i] * voxelSize);
      max[i] = Math.max(max[i], p[i] * voxelSize);
    }
  }
  return [0, 1, 2].map((i) => (min[i] + max[i]) / 2) as Vec3;
}

function projector({ wasm, view }: Bevy, [x, y]: [number, number], voxelSize: number): PointerProjector {
  return {
    onFloor(planeY) {
      const hit = wasm.intersectPlane(view, x, y, { normal: [0, 1, 0], constant: -planeY });
      return hit && [Math.round(hit[0] / voxelSize), Math.round(planeY / voxelSize), Math.round(hit[2] / voxelSize)];
    },
    heightAbove(origin, stepY) {
      const o = origin.map((v) => v * voxelSize);
      const camera = wasm.rayAtScreen(view, x, y).origin;
      const len = Math.hypot(camera[0] - o[0], camera[2] - o[2]);
      // Straight above or below the origin the facing plane is undefined.
      if (len < 1e-3) return null;
      const normal: Vec3 = [(camera[0] - o[0]) / len, 0, (camera[2] - o[2]) / len];
      const hit = wasm.intersectPlane(view, x, y, { normal, constant: -(normal[0] * o[0] + normal[2] * o[2]) });
      if (!hit) return null;
      const step = Math.max(1, stepY);
      return Math.round((hit[1] - o[1]) / (step * voxelSize)) * step;
    },
  };
}

/**
 * The room and layout editors' 3D view: a Bevy world view on the page's primary canvas, with the
 * controls' orbit camera. Picks drive `useSceneEditorTools`; the document and the tools' previews
 * go back to the view as its world and overlays.
 */
export function SceneEditorView({ backend }: { backend: SceneEditorBackend }) {
  const tools = useSceneEditorTools(backend);
  const { geometry } = useApplication();
  const kinds = useCatalog();
  const container = useRef<HTMLDivElement>(null);
  const cameraHud = useRef<HTMLDivElement>(null);
  const [bevy, setBevy] = useState<Bevy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { compiled, selection, tool, buildHeight } = backend;
  const voxelSize = compiled.cubeSize;

  useEffect(() => {
    const canvas = playCanvas();
    container.current?.append(canvas);
    let cancelled = false;
    let mounted: Bevy | null = null;
    bootBevy(canvas)
      .then((wasm) => {
        if (cancelled) return;
        const view = wasm.createView(canvas, 'world');
        wasm.setCameraMode(view, 'orbit');
        mounted = { wasm, view };
        setBevy(mounted);
      })
      .catch((e: Error) => setError(e.message));
    return () => {
      cancelled = true;
      mounted?.wasm.destroyView(mounted.view);
      canvas.remove();
    };
  }, []);

  useEffect(() => {
    if (!bevy) return;
    const meter = frameStatsMeter(() => bevy.wasm.frameStats(bevy.view), publishFrameStats);
    let frame = 0;
    const tick = (nowMs: number) => {
      meter(nowMs);
      const s = window as BevySentinels;
      s.__officexrBevyReady ||= bevy.wasm.isReady(bevy.view).ready;
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      publishFrameStats(null);
      delete (window as BevySentinels).__officexrBevyReady;
    };
  }, [bevy]);

  const sentWorld = useRef<SentWorld | null>(null);
  const { bakedLayoutPath, bakedLayoutName } = backend;
  const bakedLayouts = useMemo(
    () =>
      bakedLayoutPath
        ? [
            {
              // ponytail: Bevy fetches it without credentials, so a cross-origin studio API needs a public bake.
              gltfPath: new URL(`${bakedLayoutPath}.glb`, window.location.href).href,
              position: [0, 0, 0] as Vec3,
              rotationY: 0,
              layoutName: bakedLayoutName,
              roomInstanceId: '',
            },
          ]
        : [],
    [bakedLayoutPath, bakedLayoutName],
  );
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
  const overlays = editorOverlays({
    ghosts: tools.ghosts,
    selection: selectionBoxes,
    buildPlaneY: tool === 'add' || tool === 'tile' ? buildHeight * voxelSize : null,
    gizmo: tools.gizmo,
  });
  // Diffed by tag on the Bevy side, so resending every render is cheap.
  useEffect(() => {
    bevy?.wasm.setOverlays(bevy.view, overlays);
  });

  const instancesById = useMemo(() => new Map(compiled.instances.map((i) => [i.id, i])), [compiled.instances]);
  const downAt = useRef<Partial<Record<number, [number, number]>>>({});

  const handlePick = useEffectEvent((e: PickEvent) => {
    if (!bevy) return;
    const [x, y] = e.screen;
    const pointer = projector(bevy, e.screen, voxelSize);
    const inst = e.target.type === 'object' ? instancesById.get(e.target.id) : undefined;
    const isFloor = e.target.type === 'ground' || e.target.type === 'layout';
    const hit = (): SnapHit | null => {
      if (inst) {
        if (!e.normal || !e.point) return null;
        return {
          kind: 'cube',
          cubePosition: tools.footprintOf(inst.position, inst.kindId, inst.yawDeg).min,
          yawDeg: inst.yawDeg,
          faceNormal: e.normal,
          kindId: inst.kindId,
          point: { x: e.point[0], y: e.point[1], z: e.point[2] },
        };
      }
      if (e.target.type === 'layout' && e.point) {
        const [px, py, pz] = e.point;
        return { kind: 'floor', point: { x: px, y: py, z: pz }, ...((e.normal?.[1] ?? 0) >= UP_FACING_MIN_Y && { exactY: true }) };
      }
      if (e.target.type === 'ground') {
        const planeY = buildHeight * voxelSize;
        const p = bevy.wasm.intersectPlane(bevy.view, x, y, { normal: [0, 1, 0], constant: -planeY }) ?? e.point;
        return p && { kind: 'floor', point: { x: p[0], y: planeY, z: p[2] } };
      }
      return null;
    };
    const modKey = e.modifiers.ctrl || e.modifiers.meta;

    switch (e.kind) {
      case 'down':
        if (e.button !== null) downAt.current[e.button] = e.screen;
        if (tool === 'move' && e.button === 0 && inst) {
          tools.beginMove(inst.sourceCommandId, modKey, e.modifiers.shift, pointer);
        }
        return;
      case 'move': {
        const camera = bevy.wasm.rayAtScreen(bevy.view, x, y).origin;
        tools.cameraY.current = camera[1];
        if (cameraHud.current) {
          cameraHud.current.textContent = `camera  x:${camera[0].toFixed(2)}  y:${camera[1].toFixed(2)}  z:${camera[2].toFixed(2)}`;
        }
        if (tools.yAxis) {
          const dy = pointer.heightAbove(tools.yAxis.origin, tools.yAxis.stepY);
          if (dy !== null) tools.setYDelta(dy);
        }
        if (tool === 'delete') tools.setHoverCommandId(inst?.sourceCommandId ?? null);
        if (tool === 'add' || tool === 'tile') {
          const h = hit();
          if (h) tools.setHover(h);
        }
        return;
      }
      case 'out':
        if (tool === 'delete') tools.setHoverCommandId(null);
        if (tool === 'add' || tool === 'tile') tools.setHover(null);
        return;
      case 'drag':
        if (e.button === 0) tools.dragMove(e.modifiers.shift, pointer);
        return;
      case 'up':
      case 'dragEnd':
        if (e.button === 0) tools.endMove();
        return;
      case 'click': {
        const down = e.button === null ? undefined : downAt.current[e.button];
        if (down && Math.max(Math.abs(x - down[0]), Math.abs(y - down[1])) > CLICK_SLOP_PX) return;
        if (e.button === 2) {
          const rect = playCanvas().getBoundingClientRect();
          backend.onContextMenuRequest(inst?.sourceCommandId ?? null, rect.left + x, rect.top + y);
          return;
        }
        if (e.button !== 0) return;
        if (inst) {
          const h = hit();
          if (tool === 'select') backend.onSelectInstance(inst.sourceCommandId, modKey);
          else if (tool === 'delete') tools.deleteClick(inst.sourceCommandId);
          else if (tool === 'add' && h) tools.addClick(h);
          else if (tool === 'tile' && h) tools.tileClick(h);
        } else if (isFloor) {
          const h = hit();
          if (tool === 'select') backend.onClickEmpty();
          else if (tool === 'add' && h) tools.addClick(h);
          else if (tool === 'tile' && h) tools.tileClick(h);
        }
        return;
      }
    }
  });
  useEffect(() => {
    // Destroying the view drops its callback.
    bevy?.wasm.onPick(bevy.view, (e) => handlePick(e));
  }, [bevy]);

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      <div
        ref={container}
        style={{ position: 'absolute', inset: 0, touchAction: 'none' }}
        onContextMenu={(e) => e.preventDefault()}
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
