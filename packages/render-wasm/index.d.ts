/**
 * Hand-written types for `crates/officexr-render-web`'s wasm-bindgen API, so
 * the studio typechecks without the Bevy wasm build. Keep in step with the
 * `#[wasm_bindgen]` exports in that crate's `src/lib.rs`.
 */

export type ViewId = number;

export type ViewKind = 'world' | 'preview';

export interface CameraPose {
  position: [number, number, number];
  lookAt: [number, number, number];
  fovDeg: number;
  near: number;
  far: number;
}

/**
 * A `setWorld` update. `kinds` and `objects.instances` are upserted by id, `remove` drops objects by
 * id, and every other field present replaces the view's. A full scene is a valid diff; a view's first
 * diff must carry `lighting` and `background`.
 */
export interface WorldDiff {
  kinds?: unknown[];
  objects?: { instances: unknown[]; cubeSize?: number };
  remove?: string[];
  lighting?: unknown;
  background?: unknown;
  bakedLayouts?: { gltfPath: string; position: [number, number, number]; rotationY: number }[];
  /** A flat background instead of the sky; `null` brings the sky back. */
  clearColor?: string | null;
}

/** The wasm-bindgen module of one backend, already instantiated by `loadRenderer`. */
export interface RendererModule {
  /** Boots Bevy once, loading models from `origin`. */
  init(origin: string): void;
  /** A view drawing into `canvas` (an element, or a selector matching one). */
  createView(canvas: HTMLCanvasElement | string, kind: ViewKind): ViewId;
  /** Despawns the view's window, camera and scene, freeing its canvas for another view. */
  destroyView(id: ViewId): void;
  /** `diffJson` is a `WorldDiff`; with `mugshot`, that character is settled on its objects and drawn paused. */
  setWorld(id: ViewId, diffJson: string, mugshot?: string | null): void;
  setPlayers(id: ViewId, players: unknown): void;
  setCamera(id: ViewId, pose: CameraPose): void;
  /** `ready` once every model has loaded and settled; `pending` counts the models still loading. */
  isReady(id: ViewId): { ready: boolean; pending: number };
  frameStats(id: ViewId): { drawCalls: number; triangles: number };
}

export type Gpu = { requestAdapter(): Promise<unknown> };

export type RenderBackend = 'webgpu' | 'webgl2';

export function pickBackend(gpu?: Gpu | null): Promise<RenderBackend>;

/**
 * Loads and instantiates the renderer: WebGPU when an adapter is available, else WebGL2, from
 * `VITE_RENDER_WASM_BASE_URL` when set and from the bundled build otherwise.
 */
export function loadRenderer(): Promise<RendererModule>;
