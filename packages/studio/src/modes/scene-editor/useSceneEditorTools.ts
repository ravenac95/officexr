import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { isTypingTarget, useApplication } from '@officexr/world/react';
import { snapToVoxel, type SnapHit } from '../room/roomSnap.ts';
import { computeMovedPositions } from '../room/moveDelta.ts';
import { checkMoveOccupancy } from '../room/moveOccupancy.ts';
import { createFootprintOf } from '../room/footprint.ts';
import type { SceneEditorBackend } from './SceneEditorBackend.ts';
import type { GhostSpec } from './editorOverlays.ts';
import { nearbyObjectsOf, resolvePlacement, snapForPlacement } from './placement.ts';
import {
  computeTileGhosts,
  resolveAvailableAxes,
  switchNextAxis,
  type TileAxis,
  type TileMachineState,
} from './tileStateMachine.ts';

type Vec3 = [number, number, number];

/** The pointer projected onto the editor's construction planes, in voxels. */
export interface PointerProjector {
  /** The voxel under the pointer on the horizontal plane at world height `planeY` (m). */
  onFloor(planeY: number): Vec3 | null;
  /**
   * The pointer's height above `origin` in voxels, snapped to whole `stepY`s, on the vertical plane
   * through `origin` that faces the camera.
   */
  heightAbove(origin: Vec3, stepY: number): number | null;
}

interface MoveDrag {
  startVoxel: Vec3;
  /** Shift held at the press: a vertical drag. */
  isYAxis: boolean;
  originalPositions: ReadonlyMap<string, Vec3>;
  proposedPositions: ReadonlyMap<string, Vec3>;
  occupancyResult: 'ok' | 'blocked';
}

const AXIS_INDEX: Record<TileAxis, 0 | 1 | 2> = { x: 0, y: 1, z: 2 };
const AXIS_COLOR: Record<TileAxis, string> = { x: '#ef4444', y: '#22c55e', z: '#3b82f6' };

/**
 * The scene editor's tools — Add, Delete, Tile and Move — driven by pointer hits, with the ghosts and
 * gizmo they preview. Renderer-agnostic: the view turns its picks into the calls below.
 */
export function useSceneEditorTools(backend: SceneEditorBackend) {
  const { geometry, catalog } = useApplication();
  const footprintOf = useMemo(() => createFootprintOf(geometry), [geometry]);
  const voxelSize = backend.compiled.cubeSize;
  const { tool } = backend;

  const [hover, setHover] = useState<SnapHit | null>(null);
  const [hoverCommandId, setHoverCommandId] = useState<string | null>(null);
  const [tileState, setTileState] = useState<TileMachineState>({ stage: 'idle' });
  const [yDelta, setYDelta] = useState(0);
  // Picks arrive in per-frame batches, faster than React re-renders, so the drag lives in a ref.
  const moveDrag = useRef<MoveDrag | null>(null);
  const [moveGhosts, setMoveGhosts] = useState<GhostSpec[]>([]);
  // Biases the stack-above-or-below tie toward the viewer; the view writes it from its camera.
  const cameraY = useRef(0);

  const cancelMove = useCallback(() => {
    moveDrag.current = null;
    setMoveGhosts([]);
  }, []);

  useEffect(() => {
    if (tool !== 'tile') setTileState({ stage: 'idle' });
    if (tool !== 'move') cancelMove();
  }, [tool, cancelMove]);

  const yAxisOrigin =
    (tileState.stage === 'placed' || tileState.stage === 'axis-extruded') && tileState.nextAxis === 'y'
      ? tileState.origin
      : null;
  useEffect(() => {
    if (!yAxisOrigin) setYDelta(0);
  }, [yAxisOrigin]);

  const { onSetTool } = backend;
  useEffect(() => {
    if (tool !== 'tile' && tool !== 'move') return;
    const onKey = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target)) return;
      if (e.key === 'Escape') {
        if (tool === 'move') {
          cancelMove();
        } else {
          setTileState({ stage: 'idle' });
          onSetTool('select');
        }
        return;
      }
      // `c` cycles the remaining tile axes; per-axis keys would clash with `x` = Delete.
      if (tool !== 'tile' || e.key.toLowerCase() !== 'c') return;
      setTileState((prev) => {
        if (prev.stage === 'idle' || prev.remainingAxes.length < 2) return prev;
        const at = prev.nextAxis === null ? -1 : prev.remainingAxes.indexOf(prev.nextAxis);
        return switchNextAxis(prev, prev.remainingAxes[at < 0 ? 0 : (at + 1) % prev.remainingAxes.length]);
      });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [tool, onSetTool, cancelMove]);

  const stepForKind = useCallback(
    (kindId: string | null | undefined, yawDeg?: number) => {
      if (!kindId) return { x: 1, y: 1, z: 1 };
      const [x, y, z] = geometry.tileStep(kindId, yawDeg);
      return { x, y, z };
    },
    [geometry],
  );
  // A face hit clears the target's whole footprint along the normal.
  const targetStrideFor = useCallback(
    (hit: SnapHit) => (hit.kind === 'cube' ? stepForKind(hit.kindId, hit.yawDeg) : undefined),
    [stepForKind],
  );

  const nearbyObjects = useMemo(
    () => nearbyObjectsOf(backend.compiled.instances, footprintOf, voxelSize),
    [backend.compiled.instances, footprintOf, voxelSize],
  );
  const snapContext = () => ({ nearbyObjects, geometry, voxelSize, cameraY: cameraY.current });
  const placementFor = (hit: SnapHit, kindId: string, yawDeg: number) =>
    resolvePlacement(hit, kindId, yawDeg, {
      ...snapContext(),
      compiled: backend.compiled,
      catalog,
      footprintOf,
    });

  /** Where the tile gesture's next extrusion reaches for `hit`. */
  const tileHoverVoxel = (state: Exclude<TileMachineState, { stage: 'idle' }>, hit: SnapHit): Vec3 => {
    const v = snapToVoxel(hit, voxelSize, stepForKind(state.kindId, backend.stagedYawDeg), targetStrideFor(hit));
    return state.nextAxis === 'y' ? [v[0], state.origin[1] + yDelta, v[2]] : v;
  };

  const ghosts: GhostSpec[] = [];
  const { stagedKindId, stagedYawDeg } = backend;
  if (tool === 'add' && stagedKindId && hover) {
    const voxel = snapForPlacement(hover, stagedKindId, stagedYawDeg, snapContext());
    ghosts.push({ mode: 'solid', kindId: stagedKindId, voxel, yawDeg: stagedYawDeg });
  }
  if (tool === 'delete' && hoverCommandId) {
    const groupId = backend.commandToGroup.get(hoverCommandId);
    const targets = new Set(groupId ? (backend.groupMembers.get(groupId) ?? [hoverCommandId]) : [hoverCommandId]);
    for (const inst of backend.compiled.instances) {
      if (targets.has(inst.sourceCommandId)) {
        ghosts.push({ mode: 'pulse', kindId: inst.kindId, voxel: [...inst.position], yawDeg: inst.yawDeg });
      }
    }
  }
  if (tool === 'tile') {
    // Click 1 previews what Add would place; later clicks quantize the extrusion from the origin.
    const kindId = tileState.stage !== 'idle' ? tileState.kindId : stagedKindId;
    const hoverVoxel = !hover
      ? null
      : tileState.stage === 'idle'
        ? snapForPlacement(hover, stagedKindId, stagedYawDeg, snapContext())
        : tileHoverVoxel(tileState, hover);
    if (kindId) {
      for (const voxel of computeTileGhosts(tileState, hoverVoxel, stepForKind(kindId, stagedYawDeg))) {
        ghosts.push({ mode: 'solid', kindId, voxel, yawDeg: stagedYawDeg });
      }
    }
  }
  ghosts.push(...moveGhosts);

  const addClick = (hit: SnapHit) => {
    if (tool !== 'add' || !stagedKindId) return;
    const voxel = placementFor(hit, stagedKindId, stagedYawDeg);
    if (voxel) backend.onPlaceAt(voxel);
  };

  const deleteClick = (commandId: string) => {
    if (tool !== 'delete') return;
    backend.onDeleteCommand(commandId);
    setHoverCommandId(null);
  };

  const finishTile = () => {
    setTileState({ stage: 'idle' });
    onSetTool('select');
  };

  // Each click commits the previewed ghosts and moves on to the next of the kind's `tilingAxes`;
  // once every axis is used the tool returns to Select.
  const tileClick = (hit: SnapHit) => {
    if (tool !== 'tile' || !stagedKindId) return;
    if (tileState.stage === 'idle') {
      const voxel = placementFor(hit, stagedKindId, stagedYawDeg);
      if (!voxel) return;
      const ids = backend.onPlaceMany(stagedKindId, [voxel], null, stagedYawDeg);
      if (ids.length === 0) return;
      const kind = catalog.getKind(stagedKindId);
      const axes = kind ? resolveAvailableAxes(kind.tilingAxes) : [];
      if (axes.length === 0) {
        finishTile();
        return;
      }
      setTileState({
        stage: 'placed',
        origin: voxel,
        kindId: stagedKindId,
        originCommandId: ids[0],
        remainingAxes: axes,
        nextAxis: axes[0],
      });
      return;
    }
    const axis = tileState.nextAxis;
    if (axis === null) {
      finishTile();
      return;
    }
    const hoverVoxel = tileHoverVoxel(tileState, hit);
    const remaining = tileState.remainingAxes.filter((a) => a !== axis);
    if (hoverVoxel[AXIS_INDEX[axis]] === tileState.origin[AXIS_INDEX[axis]]) {
      if (tileState.stage === 'axis-extruded' && remaining.length === 0) finishTile();
      return;
    }
    const placed = computeTileGhosts(tileState, hoverVoxel, stepForKind(tileState.kindId, stagedYawDeg));
    if (placed.length === 0) return;
    let groupId: string;
    let extrudedRow: readonly Vec3[];
    if (tileState.stage === 'placed') {
      const newIds = backend.onPlaceMany(stagedKindId, placed, null, stagedYawDeg);
      const created = backend.onCreateGroup([tileState.originCommandId, ...newIds]);
      if (!created) return;
      groupId = created;
      extrudedRow = placed;
    } else {
      backend.onPlaceMany(stagedKindId, placed, tileState.groupId, stagedYawDeg);
      groupId = tileState.groupId;
      extrudedRow = [...tileState.extrudedRow, ...placed];
    }
    if (remaining.length === 0) {
      finishTile();
      return;
    }
    setTileState({
      stage: 'axis-extruded',
      consumedAxis: axis,
      origin: tileState.origin,
      kindId: tileState.kindId,
      groupId,
      extrudedRow,
      remainingAxes: remaining,
      nextAxis: remaining[0],
    });
  };

  /** A press on an object with the Move tool: selects an unselected one, starts dragging a selected one. */
  const beginMove = (commandId: string, modKey: boolean, shiftKey: boolean, pointer: PointerProjector) => {
    if (!backend.selection.has(commandId)) {
      backend.onSelectInstance(commandId, modKey);
      return;
    }
    const originalPositions = new Map<string, Vec3>();
    for (const id of backend.selection) {
      const cmd = backend.doc.commands.find((c) => c.id === id && c.op === 'placeObject');
      if (cmd?.op === 'placeObject') originalPositions.set(id, cmd.position);
    }
    const first = originalPositions.values().next().value;
    const startVoxel = pointer.onFloor(first ? first[1] * voxelSize : 0) ?? (first ? [...first] : [0, 0, 0]);
    moveDrag.current = {
      startVoxel,
      isYAxis: shiftKey,
      originalPositions,
      proposedPositions: originalPositions,
      occupancyResult: 'ok',
    };
  };

  const dragMove = (shiftKey: boolean, pointer: PointerProjector) => {
    const drag = moveDrag.current;
    if (!drag) return;
    let delta: Vec3;
    if (drag.isYAxis || shiftKey) {
      const dy = pointer.heightAbove(drag.startVoxel, 1);
      if (dy === null) return;
      delta = [0, dy, 0];
    } else {
      const first = drag.originalPositions.values().next().value;
      const at = pointer.onFloor((first ? first[1] : 0) * voxelSize);
      if (!at) return;
      delta = [at[0] - drag.startVoxel[0], 0, at[2] - drag.startVoxel[2]];
    }
    const proposed = computeMovedPositions(backend.doc, backend.selection, delta);
    if (!proposed) return;
    const occupancyResult = checkMoveOccupancy(backend.doc, backend.selection, proposed, footprintOf);
    moveDrag.current = { ...drag, proposedPositions: proposed, occupancyResult };
    const next: GhostSpec[] = [];
    for (const [commandId, voxel] of proposed) {
      const inst = backend.compiled.instances.find((i) => i.sourceCommandId === commandId);
      if (inst) {
        next.push({ mode: occupancyResult === 'ok' ? 'solid' : 'blocked', kindId: inst.kindId, voxel, yawDeg: inst.yawDeg });
      }
    }
    setMoveGhosts(next);
  };

  /** Commits the drag, unless its destination is blocked. */
  const endMove = () => {
    const drag = moveDrag.current;
    if (!drag) return;
    cancelMove();
    if (drag.occupancyResult === 'ok') {
      backend.onMoveSelection([...drag.proposedPositions].map(([commandId, position]) => ({ commandId, position })));
    }
  };

  const nextAxis = tileState.stage !== 'idle' ? tileState.nextAxis : null;
  const gizmo =
    tool === 'tile' && tileState.stage !== 'idle' && nextAxis
      ? {
          at: tileState.origin.map((v) => v * voxelSize) as Vec3,
          direction: [0, 1, 2].map((i) => (i === AXIS_INDEX[nextAxis] ? 1 : 0)) as Vec3,
          color: AXIS_COLOR[nextAxis],
        }
      : null;

  return {
    ghosts,
    gizmo,
    showAxisHint: tool === 'tile' && tileState.stage !== 'idle' && tileState.remainingAxes.length > 1,
    cameraY,
    /** The tile origin while the gesture extrudes along Y, with its kind's Y step. */
    yAxis: yAxisOrigin && tileState.stage !== 'idle' ? { origin: yAxisOrigin, stepY: stepForKind(tileState.kindId).y } : null,
    setYDelta,
    setHover,
    setHoverCommandId,
    addClick,
    deleteClick,
    tileClick,
    beginMove,
    dragMove,
    endMove,
    footprintOf,
  };
}
