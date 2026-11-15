import type { Overlay, WorldAabb } from '@officexr/render-wasm';
import type { VoiceZone } from '@officexr/sdk';
import { deskAreaLabel } from '@officexr/world/app';
import type { DeskArea, SpawnPoint } from '@officexr/world/scenes';
import type { DeskAreaViolation } from './deskAreaViolations.ts';
import type { MapSelection } from './useMapDocument.ts';

type Rect = Pick<DeskArea, 'min' | 'max'>;

export interface MapOverlayInputs {
  voiceZones: readonly Pick<VoiceZone, 'id' | 'min' | 'max'>[];
  deskAreas: readonly DeskArea[];
  violations: readonly DeskAreaViolation[];
  spawns: readonly SpawnPoint[];
  selection: MapSelection;
  /** World AABB of the selected room, outlined. */
  selectedRoom: WorldAabb | null;
  /** The desk area being drawn. */
  draft: Rect | null;
  /** Desk areas take clicks, which the Select tool turns into a selection. */
  deskAreasPickable: boolean;
}

const VOICE_ZONE_Y = 0.05;
const DESK_AREA_Y = 0.06;
/** `#ff00ff` at 40%. */
const VOICE_ZONE_COLOR = '#ff00ff66';
const SELECTED = '#fde047';
/** At 35%. */
const DESK_ALPHA = '59';

export const SPAWN_TAG = 'spawn:';
export const DESK_AREA_TAG = 'desk:';

const rectShape = ({ min, max }: Rect) => ({
  type: 'polygon' as const,
  points: [
    [min.x, min.z],
    [max.x, min.z],
    [max.x, max.z],
    [min.x, max.z],
  ] as [number, number][],
});

const isSelected = (selection: MapSelection, kind: NonNullable<MapSelection>['kind'], id: string) =>
  selection?.kind === kind && selection.id === id;

/** The map editor's overlays: the ground grid, voice zones, desk areas and their labels, spawns and the selected room's outline. */
export function mapOverlays(inputs: MapOverlayInputs): Overlay[] {
  const { selection } = inputs;
  const overlays: Overlay[] = [{ tag: 'grid', kind: 'grid', y: 0, cellSize: 1, fade: 80 }];
  for (const z of inputs.voiceZones) {
    overlays.push({ tag: `voice:${z.id}`, kind: 'zone', shape: rectShape(z), y: VOICE_ZONE_Y, color: VOICE_ZONE_COLOR });
  }
  for (const a of inputs.deskAreas) {
    const invalid = inputs.violations.some((v) => v.areaId === a.id);
    const color = invalid ? '#ef4444' : isSelected(selection, 'desk-area', a.id) ? SELECTED : '#22d3ee';
    overlays.push({
      tag: `${DESK_AREA_TAG}${a.id}`,
      kind: 'zone',
      shape: rectShape(a),
      y: DESK_AREA_Y,
      color: `${color}${DESK_ALPHA}`,
      pickable: inputs.deskAreasPickable,
    });
    overlays.push({
      tag: `desk-label:${a.id}`,
      kind: 'label',
      at: [(a.min.x + a.max.x) / 2, DESK_AREA_Y, (a.min.z + a.max.z) / 2],
      text: deskAreaLabel(a),
    });
  }
  if (inputs.draft) {
    overlays.push({ tag: 'desk-draft', kind: 'zone', shape: rectShape(inputs.draft), y: DESK_AREA_Y, color: `${SELECTED}${DESK_ALPHA}` });
  }
  for (const s of inputs.spawns) {
    overlays.push({
      tag: `${SPAWN_TAG}${s.id}`,
      kind: 'marker',
      shape: 'spawnCone',
      pos: s.position,
      yaw: 0,
      color: '#22d3ee',
      selected: isSelected(selection, 'spawn', s.id),
    });
  }
  if (inputs.selectedRoom) {
    overlays.push({ tag: 'room-selection', kind: 'outline', boxes: [inputs.selectedRoom], color: SELECTED });
  }
  return overlays;
}
