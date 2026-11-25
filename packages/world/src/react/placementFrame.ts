import { MAX_PLACED_PROPS, propToInstance, type OfficeState, type PlaceableKindId, type PlacedProp } from '@officexr/sdk';
import type { InstanceGeometryService } from '../app/types.ts';
import { feetPosition } from '../physics/rules.ts';
import { fitsClear, ghostSpot, PLACE_AHEAD_M, pickPlaced, type PlaceModeState, type PlacementHost } from '../motion/place-mode.ts';
import { createPlaceKeyHandler } from '../renderer/placeKeys.ts';

type Geometry = Pick<InstanceGeometryService, 'voxelSize' | 'worldAABB' | 'worldAABBOfInstance'>;

type Xyz = [number, number, number];

/** What build mode shows in the world: the chosen prop's ghost and the box round the placed prop it points at. */
export interface PlacementPreview {
  ghost: { kindId: PlaceableKindId; position: Xyz; yawDeg: number; valid: boolean } | null;
  /** World-space box of the placed prop T, G and Delete act on. */
  target: { min: Xyz; max: Xyz } | null;
}

export const NO_PLACEMENT_PREVIEW: PlacementPreview = Object.freeze({ ghost: null, target: null });

/** The ghost a view draws for `mode`; `ghost.position` is a footprint corner in voxels, like a map instance's. */
export function placementPreview(mode: PlaceModeState, placed: readonly PlacedProp[], geometry: Geometry): PlacementPreview {
  if (!mode.open || !mode.ghost) {
    return NO_PLACEMENT_PREVIEW;
  }
  const { ghost } = mode;
  const target = ghost.targetId ? placed.find((p) => p.id === ghost.targetId) : undefined;
  const box = target ? geometry.worldAABBOfInstance(propToInstance(target)) : null;
  return {
    ghost: { kindId: mode.kindId, position: ghost.position, yawDeg: mode.yawDeg, valid: ghost.valid },
    target: box && { min: [box.min[0], box.min[1], box.min[2]], max: [box.max[0], box.max[1], box.max[2]] },
  };
}

export interface PlacementFrameDeps {
  host: PlacementHost;
  getState(): OfficeState;
  geometry: Geometry;
  /** Hears build mode's keys and clicks: the window. */
  target: EventTarget;
  /** A click places only while the view has the pointer locked, as upstream's did. */
  pointerLocked(): boolean;
}

/**
 * Build mode's per-frame work in the play view: while it is open, keeps the
 * ghost 3 m ahead of the player, checks it fits, picks the placed prop the
 * player points at, and listens for the keys that place, turn, grab and
 * remove props.
 */
export class PlacementFrame {
  private stopKeys: (() => void) | null = null;
  private readonly stopMode: () => void;

  constructor(private readonly deps: PlacementFrameDeps) {
    this.stopMode = deps.host.mode.subscribe(() => this.syncKeys());
    this.syncKeys();
  }

  step(): void {
    const { host, getState, geometry } = this.deps;
    const mode = host.mode.get();
    if (!mode.open) {
      return;
    }
    const state = getState();
    const self = state.players[state.selfId];
    if (!self) {
      return;
    }
    const feet = feetPosition(self.pos, state.worldSettings.charRadius);
    const position = ghostSpot(geometry, geometry.voxelSize, feet, self.yaw, mode.kindId, mode.yawDeg);
    const placed = state.placedProps;
    host.mode.setGhost({
      position,
      valid: placed.length < MAX_PLACED_PROPS && fitsClear(geometry, placed, mode.kindId, position, mode.yawDeg),
      targetId: pickPlaced(geometry, placed, {
        x: feet.x - Math.sin(self.yaw) * PLACE_AHEAD_M,
        z: feet.z - Math.cos(self.yaw) * PLACE_AHEAD_M,
      }),
    });
  }

  dispose(): void {
    this.stopMode();
    this.stopKeys?.();
    this.stopKeys = null;
  }

  private syncKeys(): void {
    const open = this.deps.host.mode.get().open;
    if (open && !this.stopKeys) {
      this.stopKeys = this.listen();
    } else if (!open && this.stopKeys) {
      this.stopKeys();
      this.stopKeys = null;
    }
  }

  private listen(): () => void {
    const { host, getState, target, pointerLocked } = this.deps;
    const { onKeyDown, place } = createPlaceKeyHandler(host, () => getState().placedProps);
    const onKey = (e: Event) => onKeyDown(e as KeyboardEvent);
    const onMouseDown = (e: Event) => {
      if ((e as MouseEvent).button === 0 && pointerLocked()) {
        place();
      }
    };
    // Capture, so build mode claims its keys before the page's other shortcuts.
    target.addEventListener('keydown', onKey, true);
    target.addEventListener('mousedown', onMouseDown);
    const offPlace = host.mode.onPlace(place);
    return () => {
      offPlace();
      target.removeEventListener('keydown', onKey, true);
      target.removeEventListener('mousedown', onMouseDown);
    };
  }
}
