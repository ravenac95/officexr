import type { PickEvent, ViewId } from '@officexr/render-wasm';

/** An `onPick` callback that reports a click on a player's ping bell. */
export const pingBellPicks =
  (view: ViewId, onPing: (view: ViewId, playerId: string) => void) =>
  (event: PickEvent): void => {
    if (event.kind === 'click' && event.target.type === 'pingBell') {
      onPing(view, event.target.id);
    }
  };
