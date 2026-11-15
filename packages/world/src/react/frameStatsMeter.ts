import type { FrameStatsSample } from '../renderer/FrameStatsProbe.tsx';

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
  let lastMs = performance.now();
  return (nowMs) => {
    frames += 1;
    elapsedMs += nowMs - lastMs;
    lastMs = nowMs;
    if (elapsedMs >= STATS_INTERVAL_MS) {
      onSample({ fps: (1000 * frames) / elapsedMs, frameMs: elapsedMs / frames, ...renderStats() });
      frames = 0;
      elapsedMs = 0;
    }
  };
}
