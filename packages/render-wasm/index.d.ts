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
 * A `setWorld` update, applied in order: `replace` clears the kinds and objects, `remove` drops objects
 * by id, then `kinds` and `objects.instances` are upserted by id. Kinds are only ever dropped by
 * `replace`. Every other field present replaces the view's. A full scene is a valid diff; a view's
 * first diff must carry `lighting` and `background`.
 */
export interface WorldDiff {
  /** Clears the view's kinds and objects first. */
  replace?: boolean;
  kinds?: unknown[];
  objects?: { instances: unknown[]; cubeSize?: number };
  remove?: string[];
  lighting?: unknown;
  background?: unknown;
  bakedLayouts?: {
    gltfPath: string;
    position: [number, number, number];
    rotationY: number;
    /** What picks on the layout report. */
    layoutName?: string;
    roomInstanceId?: string;
  }[];
  /** A flat background instead of the sky; `null` brings the sky back. */
  clearColor?: string | null;
}

export type Vec3 = [number, number, number];

/** What a pick hit; events bubble from a mesh up to the nearest target. */
export type PickTarget =
  | { type: 'object'; id: string }
  | { type: 'layout'; name: string; roomInstanceId: string }
  | { type: 'player'; id: string }
  | { type: 'overlay'; tag: string }
  /** The infinite floor plane at y = 0. */
  | { type: 'ground' };

export type PickKind = 'over' | 'out' | 'down' | 'up' | 'click' | 'dragStart' | 'drag' | 'dragEnd';

export interface PickEvent {
  kind: PickKind;
  target: PickTarget;
  /** World-space hit; `null` on `drag` and `dragEnd`, which follow the pointer off their target. */
  point: Vec3 | null;
  normal: Vec3 | null;
  /** As `MouseEvent.button`; `null` on `over` and `out`. */
  button: 0 | 1 | 2 | null;
  modifiers: { shift: boolean; ctrl: boolean; alt: boolean; meta: boolean };
  /** CSS pixels from the canvas's top left. */
  screen: [number, number];
}

/** `normal · p + constant = 0`, as three.js's `Plane`. */
export interface Plane {
  normal: Vec3;
  constant: number;
}

/** The wasm-bindgen module of one backend, already instantiated by `loadRenderer`. */
export interface RendererModule {
  /**
   * Boots Bevy once, loading models from `origin`. A view later created on `primaryCanvas` draws into
   * Bevy's primary window directly; with WebGL2 every other view is read back and copied into its canvas.
   * The primary canvas is styled to fill its parent; it takes one live view at a time.
   */
  init(origin: string, primaryCanvas?: HTMLCanvasElement | string): void;
  /** A view drawing into `canvas` (an element, or a selector matching one). */
  createView(canvas: HTMLCanvasElement | string, kind: ViewKind): ViewId;
  /** Despawns the view's window, camera and scene, freeing its canvas for another view. The primary canvas keeps Bevy's window, idle. */
  destroyView(id: ViewId): void;
  /**
   * `diffJson` is a `WorldDiff`; with `mugshot`, that character is settled on its objects and drawn
   * paused, and the height its feet settled at is returned.
   */
  setWorld(id: ViewId, diffJson: string, mugshot?: string | null): number | undefined;
  setPlayers(id: ViewId, players: unknown): void;
  setCamera(id: ViewId, pose: CameraPose): void;
  /** `ready` once every model has loaded and settled; `pending` counts the models still loading. */
  isReady(id: ViewId): { ready: boolean; pending: number };
  frameStats(id: ViewId): { drawCalls: number; triangles: number };
  /**
   * Sends `callback` the view's pick events, from pointer events on its canvas, replacing any callback
   * it had; `null` stops them. A view without a callback does no picking.
   */
  onPick(id: ViewId, callback: ((event: PickEvent) => void) | null): void;
  /** The view camera's ray through `(x, y)`, in CSS pixels from its canvas's top left, as of the last `setCamera`. */
  rayAtScreen(id: ViewId, x: number, y: number): { origin: Vec3; dir: Vec3 };
  /** Where that ray meets `plane` ahead of the camera, or `null`. */
  intersectPlane(id: ViewId, x: number, y: number, plane: Plane): Vec3 | null;
}

export type Gpu = { requestAdapter(): Promise<unknown> };

export type RenderBackend = 'webgpu' | 'webgl2';

export function pickBackend(gpu?: Gpu | null): Promise<RenderBackend>;

/**
 * Loads and instantiates the renderer: WebGPU when an adapter is available, else WebGL2, from
 * `VITE_RENDER_WASM_BASE_URL` when set and from the bundled build otherwise.
 */
export function loadRenderer(): Promise<RendererModule>;
