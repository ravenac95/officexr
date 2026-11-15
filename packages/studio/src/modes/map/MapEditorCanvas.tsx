import React, { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import type { CameraConfig, PickEvent, Plane, WorldAabb } from '@officexr/render-wasm';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import { computeRoomVoiceZones } from '@officexr/world/app';
import {
  blobAssetPath,
  diffWorld,
  frameStatsMeter,
  setBevyReady,
  useApplication,
  useCatalog,
  useCatalogReady,
  usePrimaryBevyView,
  type BevyView,
  type SentWorld,
} from '@officexr/world/react';
import type { DeskArea, LayoutDocument, RoomDocument, SerializedMap } from '@officexr/world/scenes';
import { publishFrameStats } from '../../perf/frame-stats.ts';
import { studioBakedLayoutUrl } from '../bakedLayoutUrl.ts';
import { useBakedLayoutBlobs } from '../useBakedLayoutBlobs.ts';
import { reduceDeskAreaDrag, type DeskAreaDrag } from './deskAreaDrag.ts';
import { deskAreasOf } from './deskAreaGeometry.ts';
import type { DeskAreaViolation } from './deskAreaViolations.ts';
import { mapOverlays } from './mapOverlays.ts';
import { resolveMapPick } from './mapPointerActions.ts';
import type { MapTool } from './mapTools.ts';
import { computeRoomAabbs } from './roomAabbs.ts';
import { dragRoomTo } from './roomBoundarySnap.ts';
import { roomPlacements } from './roomPlacements.ts';
import type { MapSelection } from './useMapDocument.ts';

type Vec3 = [number, number, number];

/** Room-to-room boundary snap during a Move drag; see `roomBoundarySnap.ts`. */
const ROOM_SNAP_THRESHOLD_VOXELS = 2;
const FLOOR: Plane = { normal: [0, 1, 0], constant: 0 };

/** The old `FlyCamera`: right-drag orbits, shift+right- and middle-drag pan, WASD/QE move the target. */
const CAMERA: CameraConfig = { orbit: { moveKeys: true, minDistance: 4, maxDistance: 500 } };
const START_ORBIT = { target: [0, 0, 0] as Vec3, azimuth: -0.55, elevation: 0.7, distance: 60 };

// ponytail: shows the default world lighting, not the saved world settings
const { lighting, background } = DEFAULT_WORLD_CONSTANTS;

interface MapEditorCanvasProps {
  doc: SerializedMap;
  rooms: ReadonlyMap<string, RoomDocument>;
  layouts: ReadonlyMap<string, LayoutDocument>;
  selection: MapSelection;
  onSelect: (sel: MapSelection) => void;
  onMoveRoom: (id: string, position: Vec3) => void;
  onPlaceSpawn: (position: Vec3) => void;
  onCommitDeskArea: (bounds: Pick<DeskArea, 'min' | 'max'>) => void;
  deskAreaViolations: readonly DeskAreaViolation[];
  /** Decides what a press on a room or the floor does. */
  tool: MapTool;
}

/** A floor point under `(x, y)` in CSS pixels from the view's top left. */
function floorAt({ wasm, view }: BevyView, x: number, y: number): { x: number; z: number } | null {
  const hit = wasm.intersectPlane(view, x, y, FLOOR);
  return hit ? { x: hit[0], z: hit[2] } : null;
}

/**
 * The Map editor's 3D view: a Bevy world view on the page's primary canvas under the game's sky and
 * lighting, with the placed rooms as their objects and baked layouts, and voice zones, desk areas,
 * spawns and the selected room as overlays. Presses route through `resolveMapPick`; room drags and
 * desk-area drawing follow the pointer on the floor plane.
 */
export function MapEditorCanvas(props: MapEditorCanvasProps) {
  const { geometry, rooms: roomService } = useApplication();
  const kinds = useCatalog();
  const catalogReady = useCatalogReady();
  const container = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const bevy = usePrimaryBevyView(container, (e) => setError(e.message));
  const { rooms, layouts, tool } = props;
  // Only the LocalStorage fallback can hand the editor a v1 document; it draws empty.
  const doc = catalogReady && props.doc.schemaVersion === 2 ? props.doc : null;

  useEffect(() => {
    if (!bevy) return;
    bevy.wasm.setCameraMode(bevy.view, 'orbit');
    bevy.wasm.setCameraConfig(bevy.view, CAMERA);
    bevy.wasm.setOrbit(bevy.view, START_ORBIT);
    const meter = frameStatsMeter(() => bevy.wasm.frameStats(bevy.view), publishFrameStats);
    let frame = requestAnimationFrame(function tick(nowMs: number) {
      meter(nowMs);
      setBevyReady(bevy.wasm.isReady(bevy.view).ready);
      frame = requestAnimationFrame(tick);
    });
    return () => {
      cancelAnimationFrame(frame);
      publishFrameStats(null);
    };
  }, [bevy]);

  const placed = useMemo(
    () => (doc ? roomPlacements(doc, rooms, roomService, studioBakedLayoutUrl) : null),
    // Keyed on the placements, not the whole document, so spawn and desk edits don't recompile the map.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [doc?.rooms, rooms, roomService],
  );
  const blobs = useBakedLayoutBlobs(placed?.bakedLayouts.map((b) => b.gltfPath) ?? []);
  const bakedLayouts = useMemo(
    () =>
      (placed?.bakedLayouts ?? []).flatMap((b) => {
        const blob = blobs.get(b.gltfPath);
        return blob ? [{ ...b, gltfPath: blobAssetPath(blob, 'glb') }] : [];
      }),
    [placed, blobs],
  );
  const sentWorld = useRef<SentWorld | null>(null);
  useEffect(() => {
    if (!bevy) return;
    const { diff, sent } = diffWorld(sentWorld.current, {
      instances: placed?.instances ?? [],
      kinds,
      lighting,
      background,
      bakedLayouts,
    });
    if (!diff) return;
    bevy.wasm.setWorld(bevy.view, JSON.stringify(diff));
    sentWorld.current = sent;
  }, [bevy, placed, kinds, bakedLayouts]);

  // The rooms with their linked layouts' objects, which bound the baked GLBs; compiled once for the
  // helpers below, which each take a room service.
  const compiled = useMemo(
    () => (doc ? roomService.compileMap(doc, rooms, layouts) : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [doc?.rooms, rooms, layouts, roomService],
  );
  const { roomAabbs, voiceZones } = useMemo(() => {
    if (!doc || !compiled) return { roomAabbs: [], voiceZones: [] };
    const precompiled = { compileMap: () => compiled };
    return {
      roomAabbs: computeRoomAabbs(doc, rooms, layouts, precompiled, geometry),
      voiceZones: computeRoomVoiceZones(doc, rooms, layouts, precompiled, geometry),
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [compiled, geometry]);
  const selectedRoomId = props.selection?.kind === 'room' ? props.selection.id : null;
  const selectedRoom = useMemo((): WorldAabb | null => {
    const prefix = `${selectedRoomId}/`;
    const boxes = (compiled?.instances ?? []).filter((i) => i.id.startsWith(prefix)).map((i) => geometry.worldAABBOfInstance(i));
    if (boxes.length === 0) return null;
    const min = [0, 1, 2].map((a) => Math.min(...boxes.map((b) => b.min[a]))) as Vec3;
    const max = [0, 1, 2].map((a) => Math.max(...boxes.map((b) => b.max[a]))) as Vec3;
    return { min, max };
  }, [compiled, selectedRoomId, geometry]);

  const [draft, setDraft] = useState<Pick<DeskArea, 'min' | 'max'> | null>(null);
  const overlays = useMemo(
    () =>
      mapOverlays({
        voiceZones,
        deskAreas: deskAreasOf(props.doc),
        violations: props.deskAreaViolations,
        spawns: props.doc.spawnPoints,
        selection: props.selection,
        selectedRoom,
        draft,
        deskAreasPickable: tool === 'select',
      }),
    [voiceZones, props.doc, props.deskAreaViolations, props.selection, selectedRoom, draft, tool],
  );
  useEffect(() => {
    bevy?.wasm.setOverlays(bevy.view, overlays);
  }, [bevy, overlays]);

  /** The room being moved, and the press's floor offset from its anchor. */
  const roomDrag = useRef<{ id: string; offset: { x: number; z: number } } | null>(null);
  const handlePick = useEffectEvent((e: PickEvent) => {
    if (!bevy || e.button !== 0) return;
    if (e.kind === 'up' || e.kind === 'dragEnd') {
      roomDrag.current = null;
      return;
    }
    if (e.kind === 'drag' && roomDrag.current && doc) {
      const { id, offset } = roomDrag.current;
      const placement = doc.rooms.find((r) => r.id === id);
      const point = floorAt(bevy, e.screen[0], e.screen[1]);
      if (!placement || !point) return;
      const next = dragRoomTo(
        point,
        offset,
        placement.position,
        roomAabbs.find((a) => a.id === id) ?? null,
        roomAabbs.filter((a) => a.id !== id),
        geometry.voxelSize,
        ROOM_SNAP_THRESHOLD_VOXELS,
      );
      if (next) props.onMoveRoom(id, next);
      return;
    }
    if (e.kind !== 'down') return;
    // A press that left the canvas ends without an `up` or `dragEnd`, so each press starts afresh.
    roomDrag.current = null;
    const action = resolveMapPick(e.target, e.point, tool);
    switch (action?.type) {
      case 'select':
        props.onSelect(action.selection);
        return;
      case 'deselect':
        props.onSelect(null);
        return;
      case 'place-spawn':
        props.onPlaceSpawn(action.at);
        return;
      case 'drag-room': {
        props.onSelect({ kind: 'room', id: action.id });
        const placement = doc?.rooms.find((r) => r.id === action.id);
        if (placement) {
          const v = geometry.voxelSize;
          roomDrag.current = {
            id: action.id,
            offset: { x: action.at[0] - placement.position[0] * v, z: action.at[2] - placement.position[2] * v },
          };
        }
        return;
      }
    }
  });
  useEffect(() => {
    // Destroying the view drops its callback.
    bevy?.wasm.onPick(bevy.view, (e) => handlePick(e));
  }, [bevy]);

  // The Desk area tool draws on the floor from any press on the canvas, over rooms too, so it reads
  // the DOM's pointer rather than picks.
  const deskDrag = useRef<DeskAreaDrag>({ type: 'idle' });
  const onDeskPointer = useEffectEvent((type: 'down' | 'move' | 'up', e: PointerEvent | React.PointerEvent) => {
    const rect = container.current?.getBoundingClientRect();
    if (!bevy || !rect || tool !== 'desk-area') return;
    const result = reduceDeskAreaDrag(deskDrag.current, { type, point: floorAt(bevy, e.clientX - rect.left, e.clientY - rect.top) });
    deskDrag.current = result.drag;
    setDraft(result.drag.type === 'dragging' ? result.drag.draft : null);
    if (result.commit) props.onCommitDeskArea(result.commit);
  });
  useEffect(() => {
    const move = (e: PointerEvent) => deskDrag.current.type === 'dragging' && onDeskPointer('move', e);
    const up = (e: PointerEvent) => deskDrag.current.type === 'dragging' && onDeskPointer('up', e);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
  }, []);
  useEffect(() => {
    deskDrag.current = { type: 'idle' };
    setDraft(null);
  }, [tool]);

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      <div
        ref={container}
        style={{ position: 'absolute', inset: 0, touchAction: 'none' }}
        onContextMenu={(e) => e.preventDefault()}
        onPointerDown={(e) => e.button === 0 && onDeskPointer('down', e)}
      />
      {error && (
        <p role="alert" style={{ position: 'absolute', top: 8, left: 8, margin: 0, color: '#fff', font: '13px sans-serif' }}>
          {error}
        </p>
      )}
    </div>
  );
}
