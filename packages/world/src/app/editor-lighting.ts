import type { LightingViewConfig } from '../renderer/viewConfig.ts';

/**
 * Editor-friendly lighting defaults. Mounts the shared LightingRig
 * with shadows off (editors set `shadows={false}` on their Canvas
 * anyway), no sun disc (the editors that use these defaults draw no `SkyDome`), no aux light. Hemisphere is at 0 so the
 * `ambientFillIntensity` term acts as a flat ambient — matching the
 * look editors had before unification.
 *
 * Editors spread this and override the fields they care about
 * (sun position, intensities). The renderer's gameplay Scene gets
 * its lighting from the store's world constants — those defaults are
 * tuned for the lit world rather than an editor preview.
 */
export const DEFAULT_EDITOR_LIGHTING: LightingViewConfig = {
  sunPosition: [20, 40, 20],
  sunColor: '#ffffff',
  sunIntensity: 1.2,
  ambientIntensity: 0,
  castShadow: false,
  shadowRange: 40,
  shadowMapSize: 1024,
  shadowBias: -0.0005,
  shadowNormalBias: 0.02,
  auxLightType: 'none',
  auxIntensity: 1,
  auxDistance: 0,
  auxAngle: Math.PI / 6,
  auxPenumbra: 0.2,
  auxDecay: 2,
  showSunDisc: false,
  sunDiscRadius: 3,
  sunDiscIntensity: 2,
  ambientFillIntensity: 0.5,
};
