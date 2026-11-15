import React, { useState } from 'react';
import { expect } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { InMemoryRoomStorage, emptyRoomDocument } from '@officexr/world/scenes';
import type { SnapHit } from '../modes/room/roomSnap.ts';
import { useRoomDocument } from '../modes/room/useRoomDocument.ts';
import type { SceneEditorBackend } from '../modes/scene-editor/SceneEditorBackend.ts';
import type { EditorPick as ToolPick, PointerProjector } from '../modes/scene-editor/editorPicks.ts';
import type { Tool } from '../modes/scene-editor/tools.ts';
import { useSceneEditorTools } from '../modes/scene-editor/useSceneEditorTools.ts';
import { StudioHarness } from './StudioHarness.tsx';
import { createTestApi } from './createTestApi.ts';

type Vec3 = [number, number, number];

/** A pointer pick expressed as data: what was hit and where. */
export interface EditorPick {
  /** `empty` is a floor hit at `point` (x/z) on the build-height plane; `sky` hits nothing. */
  target: { type: 'empty' } | { type: 'sky' } | { type: 'cube'; commandId: string; hit?: SnapHit };
  point?: { x: number; z: number };
  modKey?: boolean;
}

/**
 * Headless editor: the Room document store driven through the scene editor's tools
 * (`useSceneEditorTools`), with no renderer. Drive it with `pick`, `moveSelection` and `pointerAt`.
 */
export async function renderEditorStore(
  opts: {
    stagedKindId?: string;
    stagedYawDeg?: number;
    /** Camera world-Y; stack ties resolve above it. Defaults above the scene. */
    cameraY?: number;
  } = {},
) {
  const api = createTestApi();
  const storage = new InMemoryRoomStorage({
    seed: [{ name: 'default-v2', doc: emptyRoomDocument('default-v2', 'Default') }],
  });
  const stagedKindId = opts.stagedKindId ?? null;
  const stagedYawDeg = opts.stagedYawDeg ?? 0;
  const { result } = renderHook(
    () => {
      const doc = useRoomDocument({ storage });
      const [tool, setTool] = useState<Tool>('select');
      const [buildHeight, setBuildHeight] = useState(0);
      const backend: SceneEditorBackend = {
        compiled: doc.compiled,
        selection: doc.selection,
        tool,
        stagedKindId,
        stagedYawDeg,
        buildHeight,
        commandToGroup: doc.lookup.commandToGroup,
        groupMembers: doc.lookup.groupMembers,
        doc: doc.doc,
        onPlaceAt: (position) => stagedKindId && doc.placeObject(stagedKindId, position, stagedYawDeg),
        onPlaceMany: doc.placeMany,
        onSelectInstance: (id, modKey) => (modKey ? doc.toggleFromClick(id) : doc.pickFromClick(id)),
        onDeleteCommand: doc.deleteCommand,
        onClickEmpty: doc.clearSelection,
        onCreateGroup: doc.groupCommands,
        onSetTool: setTool,
        onContextMenuRequest: () => {},
        onMoveSelection: doc.setPositionMany,
      };
      return { doc, tools: useSceneEditorTools(backend), tool, setTool, buildHeight, setBuildHeight };
    },
    { wrapper: ({ children }) => <StudioHarness api={api}>{children}</StudioHarness> },
  );
  await waitFor(() => expect(result.current.doc.roomName).toBe('default-v2'));

  const toolPick = (p: EditorPick): ToolPick => {
    const base = { cameraY: opts.cameraY ?? 10, modKey: p.modKey ?? false, shift: false };
    if (p.target.type === 'sky') return { ...base, target: null, hit: null };
    if (p.target.type === 'cube') {
      return { ...base, target: { type: 'object', commandId: p.target.commandId }, hit: p.target.hit ?? null };
    }
    const { x = 0, z = 0 } = p.point ?? {};
    const y = result.current.buildHeight * api.voxelSize;
    return { ...base, target: { type: 'floor' }, hit: { kind: 'floor', point: { x, y, z } } };
  };

  /** A pointer whose floor voxel is `floor` and whose height above any origin is `height` voxels. */
  const pointerAt = (floor: Vec3 | null, height: number | null = null): PointerProjector => ({
    onFloor: () => floor,
    heightAbove: () => height,
  });

  return {
    /** The Room document hook's latest value. */
    store: {
      get current() {
        return result.current.doc;
      },
    },
    get tools() {
      return result.current.tools;
    },
    setTool(next: Tool) {
      act(() => result.current.setTool(next));
    },
    get tool() {
      return result.current.tool;
    },
    setBuildHeight(next: number) {
      act(() => result.current.setBuildHeight(next));
    },
    pick(p: EditorPick) {
      act(() => result.current.tools.click(toolPick(p)));
    },
    hover(p: EditorPick) {
      act(() => result.current.tools.hover(toolPick(p)));
    },
    pointerAt,
    /** The pointer moved, for the tile tool's Y extrusion. */
    trackY(height: number) {
      act(() => result.current.tools.trackY(pointerAt(null, height)));
    },
    /** A Move-tool press on the first selected object, a drag by `delta` voxels, then a release. */
    moveSelection(delta: Vec3) {
      const [first] = result.current.doc.selection;
      act(() => {
        result.current.setTool('move');
      });
      act(() => result.current.tools.press(toolPick({ target: { type: 'cube', commandId: first } }), pointerAt([0, 0, 0])));
      act(() => result.current.tools.drag(delta[1] !== 0, pointerAt([delta[0], 0, delta[2]], delta[1])));
      act(() => result.current.tools.release());
    },
  };
}
