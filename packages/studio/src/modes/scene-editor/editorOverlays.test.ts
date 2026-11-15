import { describe, expect, it } from 'vitest';
import { editorOverlays, type EditorOverlayInputs } from './editorOverlays.ts';

const NONE: EditorOverlayInputs = { ghosts: [], selection: [], buildPlaneY: null, gizmo: null };

describe('editorOverlays', () => {
  it('always draws the ground grid, and nothing else for an idle editor', () => {
    expect(editorOverlays(NONE)).toEqual([{ tag: 'grid', kind: 'grid', y: 0, cellSize: 1, fade: 80 }]);
  });

  it('draws the build plane only off the ground', () => {
    expect(editorOverlays({ ...NONE, buildPlaneY: 0 }).map((o) => o.tag)).toEqual(['grid']);
    expect(editorOverlays({ ...NONE, buildPlaneY: 1.5 })).toContainEqual(
      expect.objectContaining({ tag: 'build-plane', kind: 'grid', y: 1.5 }),
    );
  });

  it('groups ghosts by mode: solid, pulsing, and blocked tinted red', () => {
    const overlays = editorOverlays({
      ...NONE,
      ghosts: [
        { mode: 'solid', kindId: 'a', voxel: [0, 0, 0] },
        { mode: 'blocked', kindId: 'a', voxel: [1, 0, 0], yawDeg: 90 },
        { mode: 'pulse', kindId: 'b', voxel: [2, 0, 0] },
        { mode: 'solid', kindId: 'b', voxel: [3, 0, 0] },
      ],
    });
    expect(overlays.slice(1)).toEqual([
      {
        tag: 'ghost-solid',
        kind: 'ghost',
        opacity: 0.5,
        objects: [
          { id: 'solid-0', kindId: 'a', position: [0, 0, 0], yawDeg: undefined },
          { id: 'solid-1', kindId: 'b', position: [3, 0, 0], yawDeg: undefined },
        ],
      },
      {
        tag: 'ghost-pulse',
        kind: 'ghost',
        opacity: 0.5,
        pulse: true,
        objects: [{ id: 'pulse-0', kindId: 'b', position: [2, 0, 0], yawDeg: undefined }],
      },
      {
        tag: 'ghost-blocked',
        kind: 'ghost',
        opacity: 0.5,
        tint: '#ef4444',
        objects: [{ id: 'blocked-0', kindId: 'a', position: [1, 0, 0], yawDeg: 90 }],
      },
    ]);
  });

  it('outlines the selection and draws the tile gizmo', () => {
    const box = { min: [0, 0, 0] as [number, number, number], max: [2, 2, 2] as [number, number, number] };
    const gizmo = { at: [1, 0, 1] as [number, number, number], direction: [0, 1, 0] as [number, number, number], color: '#22c55e' };
    expect(editorOverlays({ ...NONE, selection: [box], gizmo }).slice(1)).toEqual([
      { tag: 'selection', kind: 'outline', boxes: [box], color: '#fde68a' },
      { tag: 'tile-gizmo', kind: 'directionGizmo', ...gizmo },
    ]);
  });
});
