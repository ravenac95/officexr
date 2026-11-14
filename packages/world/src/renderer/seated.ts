import type { Vec3 } from '@officexr/sdk';

/** What the frame loop needs from the seating controller. */
export interface SeatingSource {
  /** Sits on the seat the player is touching, if any. */
  sit(): void;
  stand(): void;
  /** Installs the test `stand` uses for a collider-free spot; returns its removal. */
  setStandingCheck(isFree: (feet: Vec3) => boolean): () => void;
}
