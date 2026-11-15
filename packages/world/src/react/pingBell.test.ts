import { describe, expect, it, vi } from 'vitest';
import type { PickEvent } from '@officexr/render-wasm';
import { pingBellPicks } from './pingBell.ts';

const pick = (kind: PickEvent['kind'], target: PickEvent['target']): PickEvent => ({
  kind,
  target,
  point: null,
  normal: null,
  button: 0,
  modifiers: { shift: false, ctrl: false, alt: false, meta: false },
  screen: [0, 0],
});

describe('pingBellPicks', () => {
  it("pings the bell's player on a click, with the view", () => {
    const onPing = vi.fn();
    pingBellPicks(7 as never, onPing)(pick('click', { type: 'pingBell', id: 'bob' }));
    expect(onPing).toHaveBeenCalledExactlyOnceWith(7, 'bob');
  });

  it('ignores other picks, including a click on the player and the bell being hovered', () => {
    const onPing = vi.fn();
    const handle = pingBellPicks(7 as never, onPing);
    handle(pick('click', { type: 'player', id: 'bob' }));
    handle(pick('over', { type: 'pingBell', id: 'bob' }));
    handle(pick('down', { type: 'pingBell', id: 'bob' }));
    expect(onPing).not.toHaveBeenCalled();
  });
});
