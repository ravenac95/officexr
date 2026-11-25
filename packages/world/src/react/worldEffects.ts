import type { Store } from '@officexr/sdk';
import type { OcclusionInput, OcclusionStats, Overlay, ScreenInput } from '@officexr/render-wasm';
import type { DeskScreenLink } from '../app/desk-screens.ts';
import type { InstanceGeometryService } from '../app/types.ts';
import { placementPreview } from './placementFrame.ts';
import type { WaterVolume } from '../motion/water.ts';
import type { PlacementHost } from '../motion/place-mode.ts';
import type { CameraMode } from '../renderer/config.ts';
import type { MissionMarkerTarget } from '../renderer/missionMarker.ts';
import { DeskScreens } from './deskScreens.ts';
import { occlusionCutsAway, occlusionFades, occlusionInput } from './occlusionInput.ts';
import { missionBeacon, placementOverlays } from './playOverlays.ts';

/** The calls the world effects make on their Bevy view. */
export interface EffectsView {
  setOcclusion(input: OcclusionInput | null): void;
  occlusionStats(): OcclusionStats;
  setScreens(screens: ScreenInput[]): void;
  setOverlays(overlays: Overlay[]): void;
}

/** What the effects read from the React tree, refreshed every render. */
export interface WorldEffectsOptions {
  cameraMode: CameraMode;
  water: readonly WaterVolume[];
  deskScreens: readonly DeskScreenLink[];
  missionMarker: MissionMarkerTarget | null;
  placement?: PlacementHost;
  geometry: Pick<InstanceGeometryService, 'voxelSize' | 'worldAABB' | 'worldAABBOfInstance'>;
}

/** The test marker the occlusion e2e specs read. */
interface OcclusionMarker {
  fading: number;
  cutaway: number;
}

type MarkerWindow = { __OFFICE_OCCLUSION__?: OcclusionMarker };

/**
 * The play view's world effects, once a frame after `RoomFrame.step`: the occlusion input (objects
 * hiding the player fade, the roof over it is cut away), the desk screens' glow, and the overlays
 * (the mission beacon and the build-mode placement preview). Each is sent only when it matters:
 * occlusion every frame while it is on, the rest when they change.
 */
export class WorldEffects {
  options: WorldEffectsOptions;
  private readonly screens: DeskScreens;
  private occluding = false;
  private frame = 0;
  private sentOverlays = '[]';

  constructor(
    private readonly deps: { store: Store; selfId: string; view: EffectsView },
    options: WorldEffectsOptions,
  ) {
    this.options = options;
    this.screens = new DeskScreens(`view:${deps.selfId}`);
  }

  step(dtSec: number): void {
    this.stepOcclusion();
    this.stepScreens(dtSec);
    this.stepOverlays();
  }

  dispose(): void {
    this.screens.dispose();
    delete (globalThis as MarkerWindow).__OFFICE_OCCLUSION__;
  }

  private stepOcclusion() {
    const { store, selfId, view } = this.deps;
    const { cameraMode, water } = this.options;
    const state = store.getState();
    const input = occlusionFades(cameraMode)
      ? occlusionInput(state.players, selfId, water, state.worldSettings.charRadius, occlusionCutsAway(cameraMode))
      : null;
    if (input === null && !this.occluding) {
      return;
    }
    this.occluding = input !== null;
    view.setOcclusion(input);
    if (input === null) {
      (globalThis as MarkerWindow).__OFFICE_OCCLUSION__ = { fading: 0, cutaway: 0 };
    } else if (this.frame++ % 2 === 0) {
      const { fading, cutaway } = view.occlusionStats();
      (globalThis as MarkerWindow).__OFFICE_OCCLUSION__ = { fading, cutaway };
    }
  }

  private stepScreens(dtSec: number) {
    this.screens.setLinks(this.options.deskScreens);
    const lit = this.screens.step(this.deps.store.getState(), dtSec);
    if (lit) {
      this.deps.view.setScreens(lit);
    }
  }

  private stepOverlays() {
    const { store, selfId, view } = this.deps;
    const { missionMarker, placement, geometry } = this.options;
    const state = store.getState();
    const self = state.players[selfId]?.pos ?? null;
    const overlays = [
      ...(missionMarker ? [missionBeacon(missionMarker, self)] : []),
      ...(placement ? placementOverlays(placementPreview(placement.mode.get(), state.placedProps, geometry)) : []),
    ];
    const json = JSON.stringify(overlays);
    if (json !== this.sentOverlays) {
      this.sentOverlays = json;
      view.setOverlays(overlays);
    }
  }
}
