import React from 'react';
import { expect } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { InMemoryRoomStorage, emptyRoomDocument } from '@officexr/world/scenes';
import type { SnapHit } from '../modes/room/roomSnap.ts';
import { createFootprintOf } from '../modes/room/footprint.ts';
import { computeMovedPositions } from '../modes/room/moveDelta.ts';
import { checkMoveOccupancy } from '../modes/room/moveOccupancy.ts';
import { useRoomDocument } from '../modes/room/useRoomDocument.ts';
import { nearbyObjectsOf, resolvePlacement } from '../modes/scene-editor/placement.ts';
import type { Tool } from '../modes/scene-editor/tools.ts';
import { StudioHarness } from './StudioHarness.tsx';
import { createTestApi } from './createTestApi.ts';

/** A pointer pick expressed as data: what was hit and where. */
export interface EditorPick {
  /** `empty` is a floor hit at `point` (x/z) on the build-height plane. */
  target: { type: 'empty' } | { type: 'cube'; commandId: string; hit?: SnapHit };
  point?: { x: number; z: number };
  modKey?: boolean;
}

/**
 * Headless editor: the Room document store plus the tool dispatch the
 * canvas routes picks through, with no renderer. Drive it with `pick`.
 */
export async function renderEditorStore(opts: { stagedKindId?: string } = {}) {
  const api = createTestApi();
  const footprintOf = createFootprintOf(api.geometry);
  const storage = new InMemoryRoomStorage({
    seed: [{ name: 'default-v2', doc: emptyRoomDocument('default-v2', 'Default') }],
  });
  const { result } = renderHook(() => useRoomDocument({ storage }), {
    wrapper: ({ children }) => <StudioHarness api={api}>{children}</StudioHarness>,
  });
  await waitFor(() => expect(result.current.roomName).toBe('default-v2'));

  let tool: Tool = 'select';
  let buildHeight = 0;

  const place = (hit: SnapHit) => {
    if (!opts.stagedKindId) {
      return;
    }
    const { compiled } = result.current;
    const voxel = resolvePlacement(hit, opts.stagedKindId, 0, {
      nearbyObjects: nearbyObjectsOf(compiled.instances, footprintOf, api.voxelSize),
      geometry: api.geometry,
      voxelSize: api.voxelSize,
      compiled,
      catalog: api.catalog,
      footprintOf,
    });
    if (voxel) {
      result.current.placeObject(opts.stagedKindId, voxel);
    }
  };

  return {
    store: result,
    setTool(next: Tool) {
      tool = next;
    },
    setBuildHeight(next: number) {
      buildHeight = next;
    },
    // ponytail: select/add/delete branching mirrors the canvas pointer handlers; placement itself is the shared resolvePlacement.
    pick(p: EditorPick) {
      const doc = result.current;
      act(() => {
        if (p.target.type === 'empty') {
          if (tool === 'add') {
            const { x = 0, z = 0 } = p.point ?? {};
            place({ kind: 'floor', point: { x, y: buildHeight * api.voxelSize, z } });
          } else if (tool === 'select') {
            doc.clearSelection();
          }
          return;
        }
        const { commandId, hit } = p.target;
        if (tool === 'select') {
          if (p.modKey) {
            doc.toggleFromClick(commandId);
          } else {
            doc.pickFromClick(commandId);
          }
        } else if (tool === 'delete') {
          doc.deleteCommand(commandId);
        } else if (tool === 'add' && hit) {
          place(hit);
        }
      });
    },
    moveSelection(delta: [number, number, number]) {
      const doc = result.current;
      const moved = computeMovedPositions(doc.doc, doc.selection, delta);
      if (!moved || checkMoveOccupancy(doc.doc, doc.selection, moved, footprintOf) !== 'ok') {
        return;
      }
      act(() =>
        doc.setPositionMany([...moved].map(([commandId, position]) => ({ commandId, position }))),
      );
    },
  };
}
