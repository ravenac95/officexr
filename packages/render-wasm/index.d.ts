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
  bakedLayouts?: { gltfPath: string; position: [number, number, number]; rotationY: number }[];
  /** A flat background instead of the sky; `null` brings the sky back. */
  clearColor?: string | null;
}

export type CameraMode = 'first-person' | 'third-person' | 'fixed' | 'top-down' | 'free-fly';

/** A view's camera settings. A field left out takes its default (the TS defaults). */
export interface CameraConfig {
  fixed?: {
    azimuthDeg?: number;
    pitchDeg?: number;
    height?: number;
    maxOnScreenFrac?: number;
    minOnScreenFrac?: number;
    lateralFrac?: number;
    fov?: number;
    movementYawOffsetDeg?: number;
    /** Pins the XZ distance to the character, with no leash. */
    distanceM?: number | null;
    /** A world point to orbit and look at instead of the followed player. */
    lookAt?: [number, number, number] | null;
  };
  conversationDistance?: number;
  conversationHeight?: number;
  invertLook?: boolean;
  charRadius?: number;
}

export interface MoveIntent {
  /** World-space XZ direction, unit or zero, already turned by `yaw`. */
  dir: [number, number];
  run: boolean;
  /** True once per Space press. */
  jump: boolean;
  /** The camera yaw `dir` was turned by. */
  yaw: number;
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

  // Controls: once any of these is called on a view, its camera follows its controls, not `setCamera`.
  // Keys reach any view with input enabled; pointer lock, mouse-look and the wheel only a view
  // drawing into a Bevy window (with WebGL2, the primary canvas's).
  /** The movement since the last call, to step the sim with. */
  takeIntent(id: ViewId): MoveIntent;
  /** The local player's body root and yaw, after stepping the sim. */
  setFollowTarget(id: ViewId, pos: [number, number, number], yaw: number): void;
  setCameraMode(id: ViewId, mode: CameraMode): void;
  /** The conversation's centre, or `null` outside one. */
  setConversationFocus(id: ViewId, point: [number, number, number] | null): void;
  /** Replaces the view's camera settings. */
  setCameraConfig(id: ViewId, config: CameraConfig): void;
  /** Drive from `useWorldFocus` and `isTypingTarget`: off, keys and the mouse are ignored. */
  setInputEnabled(id: ViewId, enabled: boolean): void;
}

export type Gpu = { requestAdapter(): Promise<unknown> };

export type RenderBackend = 'webgpu' | 'webgl2';

export function pickBackend(gpu?: Gpu | null): Promise<RenderBackend>;

/**
 * Loads and instantiates the renderer: WebGPU when an adapter is available, else WebGL2, from
 * `VITE_RENDER_WASM_BASE_URL` when set and from the bundled build otherwise.
 */
export function loadRenderer(): Promise<RendererModule>;
