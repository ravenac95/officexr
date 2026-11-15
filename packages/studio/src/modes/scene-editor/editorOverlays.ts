import type { Overlay, WorldAabb } from '@officexr/render-wasm';

type Vec3 = [number, number, number];

/**
 * A tool preview: `solid` for Add / Tile / a free Move destination, `pulse` for what a Delete click
 * removes, `blocked` (red) for a Move destination that overlaps another object.
 */
export interface GhostSpec {
  mode: 'solid' | 'pulse' | 'blocked';
  kindId: string;
  voxel: Vec3;
  yawDeg?: number;
}

export interface EditorOverlayInputs {
  ghosts: readonly GhostSpec[];
  /** World AABBs of the selected objects. */
  selection: readonly WorldAabb[];
  /** World height (m) of the Add / Tile floor plane, or null to hide it. */
  buildPlaneY: number | null;
  gizmo: { at: Vec3; direction: Vec3; color: string } | null;
}

const GHOST_STYLE: Record<GhostSpec['mode'], { opacity: number; pulse?: boolean; tint?: string }> = {
  solid: { opacity: 0.5 },
  pulse: { opacity: 0.5, pulse: true },
  blocked: { opacity: 0.5, tint: '#ef4444' },
};

/** The scene editor's overlays: the ground grid, the build plane, tool ghosts, the selection outline and the tile gizmo. */
export function editorOverlays({ ghosts, selection, buildPlaneY, gizmo }: EditorOverlayInputs): Overlay[] {
  const overlays: Overlay[] = [{ tag: 'grid', kind: 'grid', y: 0, cellSize: 1, fade: 80 }];
  // ponytail: the build plane is an untinted grid; the three.js one was tinted by the height's sign and ringed at the origin.
  if (buildPlaneY !== null && buildPlaneY !== 0) {
    overlays.push({ tag: 'build-plane', kind: 'grid', y: buildPlaneY, cellSize: 2, fade: 30 });
  }
  for (const mode of ['solid', 'pulse', 'blocked'] as const) {
    const objects = ghosts
      .filter((g) => g.mode === mode)
      .map((g, i) => ({ id: `${mode}-${i}`, kindId: g.kindId, position: g.voxel, yawDeg: g.yawDeg }));
    if (objects.length > 0) {
      overlays.push({ tag: `ghost-${mode}`, kind: 'ghost', objects, ...GHOST_STYLE[mode] });
    }
  }
  if (selection.length > 0) {
    overlays.push({ tag: 'selection', kind: 'outline', boxes: [...selection], color: '#fde68a' });
  }
  if (gizmo) {
    overlays.push({ tag: 'tile-gizmo', kind: 'directionGizmo', ...gizmo });
  }
  return overlays;
}
