import { describe, expect, it } from 'vitest';
import { act } from '@testing-library/react';
import { renderEditorStore } from '../../../test-harness/renderEditorStore.tsx';

const KIND = '__primitive_blue';
const origin = { x: 0, z: 0 };

function positions(editor: Awaited<ReturnType<typeof renderEditorStore>>) {
  return editor.store.current.doc.commands.map((c) => (c.op === 'placeObject' ? c.position : null));
}

describe('scene editor store + tools (no renderer)', () => {
  it('add tool places on the floor and against a cube face without overlap; undo removes the last', async () => {
    const editor = await renderEditorStore({ stagedKindId: KIND });
    editor.setTool('add');
    editor.pick({ target: { type: 'empty' }, point: origin });
    expect(positions(editor)).toEqual([[0, 0, 0]]);

    const [first] = editor.store.current.doc.commands;
    editor.pick({
      target: {
        type: 'cube',
        commandId: first.id,
        hit: {
          kind: 'cube',
          cubePosition: [0, 0, 0],
          faceNormal: [1, 0, 0],
          kindId: KIND,
          point: { x: 0.5, y: 0.25, z: 0 },
        },
      },
    });
    expect(positions(editor)).toHaveLength(2);
    expect(positions(editor)[1]).not.toEqual([0, 0, 0]);

    act(() => editor.store.current.undo());
    expect(positions(editor)).toEqual([[0, 0, 0]]);
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
    editor.setTool('add');
    editor.pick({ target: { type: 'empty' }, point: origin });
    const [first] = positions(editor);
    // The floor click stacks below the first cube; fill that slot so the stack collides too.
    editor.pick({ target: { type: 'empty' }, point: origin });
    const [, below] = positions(editor);
    expect(below).not.toEqual(first);

    editor.pick({ target: { type: 'empty' }, point: origin });
    expect(positions(editor)).toHaveLength(2);
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
});
