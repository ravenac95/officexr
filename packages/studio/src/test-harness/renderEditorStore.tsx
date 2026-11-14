import { expect } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { InMemoryRoomStorage, emptyRoomDocument } from '@officexr/world/scenes';
import { snapToVoxel, type SnapHit } from '../modes/room/roomSnap.ts';
import { computeMovedPositions } from '../modes/room/moveDelta.ts';
import { useRoomDocument } from '../modes/room/useRoomDocument.ts';
import type { Tool } from '../modes/scene-editor/tools.ts';
import { StudioHarness } from './StudioHarness.tsx';

/** A pointer pick expressed as data: what was hit and where. */
export interface EditorPick {
  target: { type: 'empty' } | { type: 'cube'; commandId: string; hit?: SnapHit };
  point?: { x: number; y: number; z: number };
  modKey?: boolean;
}

const VOXEL_SIZE = 0.5;

/**
 * Headless editor: the Room document store plus the tool dispatch the
 * canvas routes picks through, with no renderer. Drive it with `pick`.
 */
export async function renderEditorStore(opts: { stagedKindId?: string } = {}) {
  const storage = new InMemoryRoomStorage({
    seed: [{ name: 'default-v2', doc: emptyRoomDocument('default-v2', 'Default') }],
  });
  const { result } = renderHook(() => useRoomDocument({ storage }), { wrapper: StudioHarness });
  await waitFor(() => expect(result.current.roomName).toBe('default-v2'));

  let tool: Tool = 'select';
  const editor = {
    store: result,
    setTool(next: Tool) {
      tool = next;
    },
    pick(p: EditorPick) {
      const doc = result.current;
      act(() => {
        if (p.target.type === 'empty') {
          if (tool === 'add' && opts.stagedKindId) {
            doc.placeObject(
              opts.stagedKindId,
              snapToVoxel({ kind: 'floor', point: p.point ?? { x: 0, y: 0, z: 0 } }, VOXEL_SIZE),
            );
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
        } else if (tool === 'add' && hit && opts.stagedKindId) {
          doc.placeObject(opts.stagedKindId, snapToVoxel(hit, VOXEL_SIZE));
        }
      });
    },
    moveSelection(delta: [number, number, number]) {
      const doc = result.current;
      const moved = computeMovedPositions(doc.doc, doc.selection, delta);
      if (!moved) {
        return;
      }
      act(() =>
        doc.setPositionMany([...moved].map(([commandId, position]) => ({ commandId, position }))),
      );
    },
  };
  return editor;
}
