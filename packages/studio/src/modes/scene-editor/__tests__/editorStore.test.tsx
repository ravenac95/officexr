import { describe, expect, it } from 'vitest';
import { act } from '@testing-library/react';
import { renderEditorStore } from '../../../test-harness/renderEditorStore.tsx';

const KIND = '__primitive_blue';
/** A 2 m catalog block that tiles on every axis. */
const TILED = 'colored_block_blue';

const pickOn = (commandId: string) => ({
  target: { type: 'object' as const, commandId },
  hit: null,
  cameraY: 10,
  modKey: false,
  shift: false,
});
const origin = { x: 0, z: 0 };

function faceHit(commandId: string, faceNormal: [number, number, number], point: { x: number; y: number; z: number }) {
  return {
    target: {
      type: 'cube' as const,
      commandId,
      hit: { kind: 'cube' as const, cubePosition: [0, 0, 0] as [number, number, number], faceNormal, kindId: KIND, point },
    },
  };
}

function positions(editor: Awaited<ReturnType<typeof renderEditorStore>>) {
  return editor.store.current.doc.commands.map((c) => (c.op === 'placeObject' ? c.position : null));
}

describe('scene editor store + tools (no renderer)', () => {
  it('add tool places on the floor and against cube faces; undo removes the last', async () => {
    const editor = await renderEditorStore({ stagedKindId: KIND });
    editor.setTool('add');
    editor.pick({ target: { type: 'empty' }, point: origin });
    expect(positions(editor)).toEqual([[0, 0, 0]]);

    const [first] = editor.store.current.doc.commands;
    editor.pick(faceHit(first.id, [1, 0, 0], { x: 2, y: 0.2, z: 1 }));
    expect(positions(editor)).toEqual([[0, 0, 0], [4, 0, 0]]);

    editor.pick(faceHit(first.id, [0, 1, 0], { x: 1, y: 2, z: 1 }));
    expect(positions(editor)).toEqual([[0, 0, 0], [4, 0, 0], [0, 4, 0]]);

    act(() => editor.store.current.undo());
    expect(positions(editor)).toEqual([[0, 0, 0], [4, 0, 0]]);
  });

  it('select tool selects, multi-selects with the modifier, and clears on empty', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [3, 0, 0]]);
    });

    editor.pick({ target: { type: 'cube', commandId: ids[0] } });
    expect([...editor.store.current.selection]).toEqual([ids[0]]);

    editor.pick({ target: { type: 'cube', commandId: ids[1] }, modKey: true });
    expect([...editor.store.current.selection].sort()).toEqual([...ids].sort());

    editor.pick({ target: { type: 'empty' }, point: origin });
    expect(editor.store.current.selection.size).toBe(0);
  });

  it('move moves the selection as one undo step', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [3, 0, 0]]);
    });
    act(() => editor.store.current.setSelection(ids));

    editor.moveSelection([0, 0, 2]);
    expect(positions(editor)).toEqual([[0, 0, 2], [3, 0, 2]]);

    act(() => editor.store.current.undo());
    expect(positions(editor)).toEqual([[0, 0, 0], [3, 0, 0]]);
  });

  it('move is refused when it would land in an occupied voxel', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [2, 0, 0]]);
    });
    act(() => editor.store.current.setSelection([ids[0]]));

    editor.moveSelection([2, 0, 0]);
    expect(positions(editor)).toEqual([[0, 0, 0], [2, 0, 0]]);
  });

  it('add tool refuses a placement that still overlaps after stacking', async () => {
    const editor = await renderEditorStore({ stagedKindId: KIND });
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [4, 0, 0]]);
    });
    editor.setTool('add');
    // The +x slot is taken, so each click stacks above/below it until
    // those slots are full and the overlap guard refuses further clicks.
    const counts: number[] = [];
    for (let i = 0; i < 4; i++) {
      editor.pick(faceHit(ids[0], [1, 0, 0], { x: 2, y: 0.2, z: 1 }));
      counts.push(positions(editor).length);
    }
    expect(counts).toEqual([3, 4, 5, 5]);
    expect(positions(editor)).toEqual([[0, 0, 0], [4, 0, 0], [2, 4, 0], [2, -4, 0], [2, 8, 0]]);
  });

  it('delete tool removes the picked cube and undo restores it', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [3, 0, 0]]);
    });
    editor.setTool('delete');
    editor.pick({ target: { type: 'cube', commandId: ids[0] } });
    expect(positions(editor)).toEqual([[3, 0, 0]]);

    act(() => editor.store.current.undo());
    expect(positions(editor)).toEqual([[0, 0, 0], [3, 0, 0]]);
  });

  it('delete cascades through a group', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [1, 0, 0]]);
      editor.store.current.groupCommands(ids);
    });
    editor.setTool('delete');
    editor.pick({ target: { type: 'cube', commandId: ids[1] } });
    expect(positions(editor)).toEqual([]);
  });

  it('add tool previews a ghost where a click would place', async () => {
    const editor = await renderEditorStore({ stagedKindId: KIND });
    editor.setTool('add');
    editor.hover({ target: { type: 'empty' }, point: origin });
    expect(editor.tools.ghosts).toEqual([{ mode: 'solid', kindId: KIND, voxel: [0, 0, 0], yawDeg: 0 }]);
  });

  it('move tool: a press on an unselected object selects it instead of dragging', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0]]);
    });
    editor.setTool('move');
    act(() => editor.tools.press(pickOn(ids[0]), editor.pointerAt([0, 0, 0])));
    act(() => editor.tools.drag(false, editor.pointerAt([2, 0, 0])));
    act(() => editor.tools.release());
    expect([...editor.store.current.selection]).toEqual([ids[0]]);
    expect(positions(editor)).toEqual([[0, 0, 0]]);
  });

  it('move tool: a cancelled drag commits nothing, and a later release has nothing to commit', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0]]);
    });
    act(() => editor.store.current.setSelection(ids));
    editor.setTool('move');
    act(() => editor.tools.press(pickOn(ids[0]), editor.pointerAt([0, 0, 0])));
    act(() => editor.tools.drag(false, editor.pointerAt([0, 0, 3])));
    expect(editor.tools.ghosts).toEqual([{ mode: 'solid', kindId: KIND, voxel: [0, 0, 3], yawDeg: undefined }]);
    act(() => editor.tools.cancelDrag());
    act(() => editor.tools.release());
    expect(positions(editor)).toEqual([[0, 0, 0]]);
    expect(editor.tools.ghosts).toEqual([]);
  });

  it('move tool: a drag onto another object shows a blocked ghost and commits nothing', async () => {
    const editor = await renderEditorStore();
    let ids: string[] = [];
    act(() => {
      ids = editor.store.current.placeMany(KIND, [[0, 0, 0], [2, 0, 0]]);
    });
    act(() => editor.store.current.setSelection([ids[0]]));
    editor.setTool('move');
    act(() => editor.tools.press(pickOn(ids[0]), editor.pointerAt([0, 0, 0])));
    act(() => editor.tools.drag(false, editor.pointerAt([2, 0, 0])));
    expect(editor.tools.ghosts.map((g) => g.mode)).toEqual(['blocked']);
    act(() => editor.tools.release());
    expect(positions(editor)).toEqual([[0, 0, 0], [2, 0, 0]]);
  });

  it('tile tool commits a Y extrusion with the pointer over nothing', async () => {
    const editor = await renderEditorStore({ stagedKindId: TILED });
    editor.setTool('tile');
    editor.pick({ target: { type: 'empty' }, point: origin });
    expect(positions(editor)).toEqual([[0, 0, 0]]);
    // The kind tiles X → Z → Y; `c` twice makes Y next.
    for (let i = 0; i < 2; i++) {
      act(() => {
        window.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' }));
      });
    }
    expect(editor.tools.yAxis?.origin).toEqual([0, 0, 0]);
    editor.trackY(4);
    editor.pick({ target: { type: 'sky' } });
    expect(positions(editor)).toEqual([[0, 0, 0], [0, 4, 0]]);
    expect(Object.keys(editor.store.current.doc.groups)).toHaveLength(1);
  });
});
