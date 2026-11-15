import { describe, expect, it, vi } from 'vitest';
import { createMultiClientHarness } from '@officexr/sdk/test-harness';
import { FrameLoop, MAX_DT_SEC, type FrameScheduler } from './frameLoop.ts';

/** Runs frames on demand instead of on the display's refresh. */
function manualScheduler() {
  let pending: ((nowMs: number) => void) | null = null;
  const scheduler: FrameScheduler = {
    request: (cb) => {
      pending = cb;
      return 1;
    },
    cancel: () => {
      pending = null;
    },
  };
  return {
    scheduler,
    run(nowMs: number) {
      const cb = pending;
      pending = null;
      cb?.(nowMs);
    },
    get scheduled() {
      return pending !== null;
    },
  };
}

async function session() {
  const alice = await createMultiClientHarness().add('alice');
  return { actions: alice.actions, store: alice.store, rules: alice.rules, bus: alice.bus, sync: alice.sync, handshake: alice.handshake };
}

describe('FrameLoop', () => {
  it("runs the view's frame, then ticks the session, every frame", async () => {
    const deps = await session();
    const order: string[] = [];
    vi.spyOn(deps.actions, 'tick').mockImplementation(() => order.push('tickSession'));
    const frames = manualScheduler();
    const loop = new FrameLoop(deps, () => order.push('frame'), { scheduler: frames.scheduler });

    loop.start();
    frames.run(1000);
    frames.run(1016);

    expect(order).toEqual(['frame', 'tickSession', 'frame', 'tickSession']);
  });

  it('starts at dt 0 and caps a long gap', async () => {
    const dts: number[] = [];
    const frames = manualScheduler();
    const loop = new FrameLoop(await session(), (dtSec) => dts.push(dtSec), { scheduler: frames.scheduler });

    loop.start();
    frames.run(1000);
    frames.run(1016);
    frames.run(6000);

    expect(dts).toEqual([0, 0.016, MAX_DT_SEC]);
  });

  it('reports a throwing frame and still ticks the session', async () => {
    const deps = await session();
    const ticks = vi.spyOn(deps.actions, 'tick');
    const errors: Error[] = [];
    const frames = manualScheduler();
    const loop = new FrameLoop(
      deps,
      () => {
        throw new Error('boom');
      },
      { scheduler: frames.scheduler, onError: (e) => errors.push(e) },
    );

    loop.start();
    frames.run(1000);

    expect(errors.map((e) => e.message)).toEqual(['boom']);
    expect(ticks).toHaveBeenCalledWith(1000);
  });

  it('schedules no frame once stopped', async () => {
    const frame = vi.fn();
    const frames = manualScheduler();
    const loop = new FrameLoop(await session(), frame, { scheduler: frames.scheduler });

    loop.start();
    frames.run(1000);
    loop.stop();

    expect(frames.scheduled).toBe(false);
    expect(frame).toHaveBeenCalledTimes(1);
  });
});
