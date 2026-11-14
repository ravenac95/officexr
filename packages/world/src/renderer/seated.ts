import type { Vec3 } from '@officexr/sdk';

/** What the frame loop needs from the seating controller. */
export interface SeatingSource {
  /** Sits on the seat the player is touching, if any. */
  sit(): void;
  stand(): void;
  /** Installs the test `stand` uses for a collider-free spot; returns its removal. */
  setStandingCheck(isFree: (feet: Vec3) => boolean): () => void;
}

/** Sits on the frame E goes down; returns whether E is held, for the next frame's call. */
export function sitOnPress(keys: ReadonlySet<string>, wasDown: boolean, seating: SeatingSource | undefined): boolean {
  const down = keys.has('e');
  if (down && !wasDown) {
    seating?.sit();
  }
  return down;
}
