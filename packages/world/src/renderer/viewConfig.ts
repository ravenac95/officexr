import type { BackgroundConstants, FixedCameraConstants, LightingConstants } from '@officexr/sdk';

export type BackgroundViewConfig = BackgroundConstants;

export type LightingViewConfig = LightingConstants & {
  /** Flat white ambient added on top of the sun and hemisphere, unlike
   * `ambientIntensity` (the hemisphere), so shadowed faces lift without
   * changing the sky/ground tint. Editor and mugshot views set it;
   * undefined or 0 adds nothing. */
  ambientFillIntensity?: number;
};

/** The fixed camera's world constants plus the per-view framing a host can override. */
export type FixedCameraViewConfig = FixedCameraConstants & {
  /** Keeps the local player's chest at the screen centre. */
  centred?: boolean;
};
