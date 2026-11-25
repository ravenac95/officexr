// @vitest-environment jsdom
import { createRef } from 'react';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it } from 'vitest';
import { createMultiClientHarness } from '@officexr/sdk/test-harness';
import { BELL_SPACE_PX, PlayerOverheads, placeStacks, type PlayerOverheadsHandle } from './PlayerOverheads.tsx';
import { frozenAvatarId } from './roomFrame.ts';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('placeStacks', () => {
  it('moves each stack to its anchor, above a shown bell, and hides one without an anchor', () => {
    const [a, b, c] = [0, 1, 2].map(() => document.createElement('div'));
    const stacks = new Map([
      ['a', a!],
      ['b', b!],
      ['c', c!],
    ]);
    placeStacks(stacks, [
      { id: 'a', x: 10, y: 50, height: 2, bell: false },
      { id: 'b', x: 20, y: 60, height: 2, bell: true },
      { id: 'c', x: null, y: null, height: 2, bell: false },
    ]);
    expect(a!.style.transform).toBe('translate3d(10.0px, 50.0px, 0)');
    expect(b!.style.transform).toBe(`translate3d(20.0px, ${(60 - BELL_SPACE_PX).toFixed(1)}px, 0)`);
    expect(a!.style.visibility).toBe('');
    expect(c!.style.visibility).toBe('hidden');
    placeStacks(stacks, []);
    expect(a!.style.visibility).toBe('hidden');
  });
});

describe('PlayerOverheads', () => {
  it("shows a peer's name and status, and an away claimant's tag that opens their card", async () => {
    const harness = createMultiClientHarness();
    const alice = await harness.add('alice');
    alice.actions.upsertPlayer({ id: 'bob', name: 'Bob', statusText: 'Lunch' });
    const host = document.body.appendChild(document.createElement('div'));
    const root = createRoot(host);
    const handle = createRef<PlayerOverheadsHandle>();
    const pinged: string[] = [];
    await act(async () => {
      root.render(
        <PlayerOverheads
          ref={handle}
          store={alice.store}
          bus={alice.bus}
          selfId="alice"
          frozen={[{ login: 'cat', deskId: 'd', name: 'Cat', pos: { x: 0, y: 0, z: 0 }, yaw: 0, model: { url: 'm', ghost: false, fallbackUrl: 'm' } }]}
          onPingFrozen={(login) => pinged.push(login)}
        />,
      );
    });
    expect(host.textContent).toContain('Bob');
    expect(host.textContent).toContain('Lunch');
    expect(host.textContent).toContain('Cat');
    expect(host.querySelector('[role="dialog"]')).toBeNull();

    await act(async () => {
      handle.current!.toggleFrozen('cat');
      handle.current!.place([{ id: frozenAvatarId('cat'), x: 5, y: 5, height: 2, bell: false }]);
    });
    expect(host.querySelector('[role="dialog"]')).not.toBeNull();
    await act(async () => {
      host.querySelector<HTMLButtonElement>('.frozen-card-actions button')!.click();
    });
    expect(pinged).toEqual(['cat']);
    root.unmount();
    host.remove();
  });
});
