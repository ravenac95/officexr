export interface DebugFpsSample {
  fps: number;
  frameMs: number;
  drawCalls: number;
  triangles: number;
}

/** A corner readout of the play view's frame rate, and the renderer backend it booted. */
export function DebugFps({ sample }: { sample: DebugFpsSample | null }) {
  const backend = (globalThis as { __officexrRenderBackend?: string }).__officexrRenderBackend ?? '…';
  return (
    <div
      data-testid="debug-fps"
      className="pointer-events-none fixed bottom-2 left-2 z-50 rounded bg-black/70 px-2 py-1 font-mono text-xs text-white"
    >
      {sample
        ? `${sample.fps.toFixed(0)} fps · ${sample.frameMs.toFixed(1)} ms · ${sample.drawCalls} draws · ${backend}`
        : `— fps · ${backend}`}
    </div>
  );
}
