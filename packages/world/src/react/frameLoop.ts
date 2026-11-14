import { tickSession, type OfficeState } from '@officexr/sdk';

export type FrameLoopSession = Omit<Parameters<typeof tickSession>[0], 'prevState' | 'nowMs'>;

export interface FrameScheduler {
  request(cb: (nowMs: number) => void): number;
  cancel(handle: number): void;
}

/** Caps the step after a backgrounded tab resumes. */
export const MAX_DT_SEC = 0.1;

const animationFrames: FrameScheduler = {
  request: (cb) => requestAnimationFrame(cb),
  cancel: (handle) => cancelAnimationFrame(handle),
};

export interface FrameLoopOptions {
  scheduler?: FrameScheduler;
  /** Receives what the view's frame work throws; the session still ticks that frame. */
  onError?: (error: Error) => void;
}

/**
 * A play view's `requestAnimationFrame` loop: each frame runs the view's own
 * `frame` work with a capped dt, then ticks the session (rules, sync flush,
 * handshake timers) on the state that work left behind.
 */
export class FrameLoop {
  private prevState: OfficeState | null = null;
  private lastMs: number | null = null;
  private handle: number | null = null;

  private readonly scheduler: FrameScheduler;

  constructor(
    private readonly session: FrameLoopSession,
    private readonly frame: (dtSec: number, nowMs: number) => void,
    private readonly options: FrameLoopOptions = {},
  ) {
    this.scheduler = options.scheduler ?? animationFrames;
  }

  start(): void {
    if (this.handle === null) {
      this.prevState = this.session.store.getState();
      this.handle = this.scheduler.request(this.tick);
    }
  }

  stop(): void {
    if (this.handle !== null) {
      this.scheduler.cancel(this.handle);
      this.handle = null;
    }
    this.lastMs = null;
  }

  private readonly tick = (nowMs: number): void => {
    this.handle = this.scheduler.request(this.tick);
    const dtSec = this.lastMs === null ? 0 : Math.min(Math.max((nowMs - this.lastMs) / 1000, 0), MAX_DT_SEC);
    this.lastMs = nowMs;
    try {
      this.frame(dtSec, nowMs);
    } catch (error) {
      this.options.onError?.(error instanceof Error ? error : new Error(String(error)));
    }
    this.prevState = tickSession({
      ...this.session,
      prevState: this.prevState ?? this.session.store.getState(),
      nowMs,
    });
  };
}
