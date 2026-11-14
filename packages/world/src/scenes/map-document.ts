/**
 * Map authoring document. A Map composes one or more Rooms — each
 * room is a separately-authored `RoomDocument` (see `commands.ts`) —
 * into a single playable world. The map adds:
 *
 *   - per-instance `RoomInstance` placement (voxel-coord offset +
 *     quarter-turn yaw)
 *   - `SpawnPoint`s in continuous world coords for character spawns
 *
 * Runtime: `compileMap()` (studio-side) recompiles each referenced
 * RoomDocument, offsets cube positions by the RoomInstance position,
 * applies the quarter-turn rotation, namespaces ids, and emits a flat
 * `WorldObjects` the SDK store can broadcast.
 */

/**
 * One placement of a room in a map. Multiple `RoomInstance`s can
 * reference the same `roomName` so the same authored room can be
 * placed twice (e.g. mirrored east/west wings).
 *
 * `position` is in voxel coords, the unit room commands use.
 * `rotationY` is a quarter turn (0|1|2|3) about +Y around the room origin.
 */
export interface RoomInstance {
  id: string;
  roomName: string;
  position: [number, number, number];
  rotationY?: 0 | 1 | 2 | 3;
  /** Everyone inside this placed room shares one voice + screenshare call. */
  voiceZone?: boolean;
  /** Part of the main office building; its footprint bounds 2D-mode movement. */
  building?: boolean;
}

/**
 * A character spawn point in CONTINUOUS WORLD COORDS. Spawn points
 * are gameplay markers, not voxel structures, so they don't need to
 * snap to the cube grid — a designer can place a spawn between cubes
 * or above the floor as needed.
 */
export interface SpawnPoint {
  id: string;
  label?: string;
  position: [number, number, number];
}

/** An XZ rectangle (world metres) that is a voice zone. An area with more than one chair or more than one desk has no seat. */
export interface DeskArea {
  id: string;
  label?: string;
  min: { x: number; z: number };
  max: { x: number; z: number };
}

export interface MapDocumentV1 {
  schemaVersion: 1;
  name: string;
  title?: string;
  updatedAt?: number;
  rooms: RoomInstance[];
  spawnPoints: SpawnPoint[];
  deskAreas?: DeskArea[];
}

/** Same shape as v1, but a placement's position and rotation now apply to baked layouts too. */
export interface MapDocumentV2 extends Omit<MapDocumentV1, 'schemaVersion'> {
  schemaVersion: 2;
}

/** Any map version found on disk. */
export type SerializedMap = MapDocumentV1 | MapDocumentV2;

export function emptyMapDocument(name: string, title?: string): MapDocumentV2 {
  return {
    schemaVersion: 2,
    name,
    title,
    updatedAt: Date.now(),
    rooms: [],
    spawnPoints: [],
  };
}
