import type { PickTarget } from '@officexr/render-wasm';
import { assertNever } from '../../assertNever.ts';
import { DESK_AREA_TAG, SPAWN_TAG } from './mapOverlays.ts';
import type { MapTool } from './mapTools.ts';
import type { MapSelection } from './useMapDocument.ts';

/**
 * What a left-pointer-down on a room should do, given the active tool.
 *
 *   - `place-spawn`  — Spawn tool: drop a spawn at the raycast hit.
 *   - `select`       — Select tool: select the room, no drag.
 *   - `select-drag`  — Move tool: select the room AND begin a drag.
 *   - `ignore`       — Desk-area tool: the drawing layer owns the gesture.
 *
 * IMPORTANT (taxonomy F regression guard): this decision depends ONLY
 * on the active tool. It deliberately does NOT branch on the hit mesh's
 * `userData` (e.g. `isObjectInstanceMesh`). That gate was the c606f10
 * regression — once rooms started rendering a `BakedLayout` mesh in
 * front of the raw cubes, the flag-gated handler early-returned on the
 * baked-layout hit and silently broke drag + spawn. Routing purely off
 * the tool means any descendant of the room group (cube OR baked
 * layout) behaves identically.
 */
export type RoomPointerAction = 'place-spawn' | 'select' | 'select-drag' | 'ignore';

export function resolveRoomPointerAction(tool: MapTool): RoomPointerAction {
  switch (tool) {
    case 'spawn':
      return 'place-spawn';
    case 'move':
      return 'select-drag';
    case 'select':
      return 'select';
    case 'desk-area':
      return 'ignore';
    default:
      return assertNever(tool);
  }
}

type Vec3 = [number, number, number];

/** What a left press on a pick target does in the map editor. */
export type MapPickAction =
  | { type: 'select'; selection: NonNullable<MapSelection> }
  | { type: 'deselect' }
  | { type: 'place-spawn'; at: Vec3 }
  /** Selects the room and starts dragging it from `at`. */
  | { type: 'drag-room'; id: string; at: Vec3 };

/** The room placement a pick on its baked layout or one of its objects (`<placement id>/…`) belongs to. */
function roomOf(target: PickTarget): string | null {
  if (target.type === 'layout') return target.roomInstanceId || null;
  if (target.type === 'object') return target.id.split('/')[0] || null;
  return null;
}

/**
 * Routes a left press by what it hit. Spawn markers and desk areas select themselves; rooms follow
 * `resolveRoomPointerAction`; bare ground deselects, except in the Spawn and Desk area tools, where
 * spawns must land on a room and the drawing gesture owns the press.
 */
export function resolveMapPick(target: PickTarget, point: Vec3 | null, tool: MapTool): MapPickAction | null {
  if (target.type === 'overlay') {
    if (target.tag.startsWith(SPAWN_TAG)) return { type: 'select', selection: { kind: 'spawn', id: target.tag.slice(SPAWN_TAG.length) } };
    if (target.tag.startsWith(DESK_AREA_TAG)) return { type: 'select', selection: { kind: 'desk-area', id: target.tag.slice(DESK_AREA_TAG.length) } };
    return null;
  }
  if (target.type === 'ground') {
    return tool === 'select' || tool === 'move' ? { type: 'deselect' } : null;
  }
  const room = roomOf(target);
  if (!room || !point) return null;
  const action = resolveRoomPointerAction(tool);
  switch (action) {
    case 'place-spawn':
      return { type: 'place-spawn', at: point };
    case 'select':
      return { type: 'select', selection: { kind: 'room', id: room } };
    case 'select-drag':
      return { type: 'drag-room', id: room, at: point };
    case 'ignore':
      return null;
    default:
      return assertNever(action);
  }
}
