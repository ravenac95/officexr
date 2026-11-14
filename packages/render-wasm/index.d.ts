/**
 * Hand-written types for `crates/officexr-render-web`'s wasm-bindgen API, so
 * the studio typechecks without the Bevy wasm build. Keep in step with the
 * `#[wasm_bindgen]` exports in that crate's `src/lib.rs`.
 */

/** The wasm-bindgen module of one backend, already instantiated by `loadRenderer`. */
export interface RendererModule {
  /** Boots Bevy on the canvas matching `canvas`, loading models from `origin`. */
  start(canvas: string, origin: string): void;
  /** Replaces the scene; with `mugshot`, that character is drawn paused on it. */
  setScene(sceneJson: string, camera: unknown, mugshot?: string | null): void;
  setPlayers(players: unknown): void;
  /** Every model of the last scene has loaded and settled. */
  isReady(): boolean;
}

export type Gpu = { requestAdapter(): Promise<unknown> };

export type RenderBackend = 'webgpu' | 'webgl2';

export function pickBackend(gpu?: Gpu | null): Promise<RenderBackend>;

/**
 * Loads and instantiates the renderer: WebGPU when an adapter is available, else WebGL2, from
 * `VITE_RENDER_WASM_BASE_URL` when set and from the bundled build otherwise.
 */
export function loadRenderer(): Promise<RendererModule>;
