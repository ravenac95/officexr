/** One aggregated sample of a view's render performance. */
export interface FrameStatsSample {
  /** Frames completed per second, averaged over the sample window. */
  fps: number;
  /** Mean frame-to-frame delta over the window, in milliseconds. */
  frameMs: number;
  /** Longest single frame delta in the window, in milliseconds. */
  worstMs: number;
  /** Draw calls in the last rendered frame. */
  drawCalls: number;
  /** Triangles drawn in the last rendered frame. */
  triangles: number;
}

const STATS_INTERVAL_MS = 500;

/**
 * Call the returned function once a frame: every 500 ms it reports the real frame time since the last
 * report, with `renderStats` (a Bevy view's `frameStats`).
 */
export function frameStatsMeter(
  renderStats: () => { drawCalls: number; triangles: number },
  onSample: (sample: FrameStatsSample) => void,
): (nowMs: number) => void {
  let frames = 0;
  let elapsedMs = 0;
  let worstMs = 0;
  let lastMs = performance.now();
  return (nowMs) => {
    const deltaMs = nowMs - lastMs;
    frames += 1;
    elapsedMs += deltaMs;
    worstMs = Math.max(worstMs, deltaMs);
    lastMs = nowMs;
    if (elapsedMs >= STATS_INTERVAL_MS) {
      onSample({ fps: (1000 * frames) / elapsedMs, frameMs: elapsedMs / frames, worstMs, ...renderStats() });
      frames = 0;
      elapsedMs = 0;
      worstMs = 0;
    }
  };
}
