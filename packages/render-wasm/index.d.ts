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

/**
 * - `over` / `out`: the pointer starts or stops hovering a target, including when it leaves the canvas.
 *   A target that is despawned while hovered gets no `out`.
 * - `move`: the pointer moved over a target, with this move's hit, so it follows faces and the ground.
 * - `click`: a press and release on the same target. It still fires after a drag on that target, so
 *   consumers that drag skip the `click` that follows a `dragEnd` themselves.
 * - `dragStart` / `drag` / `dragEnd`: a pressed pointer moving; all go to the target the press was on.
 *   A leave or `pointercancel` mid-press ends it without `up`, `click` or `dragEnd`.
 */
export type PickKind =
  | 'over'
  | 'out'
  | 'move'
  | 'down'
  | 'up'
  | 'click'
  | 'dragStart'
  | 'drag'
  | 'dragEnd';

export interface PickEvent {
  kind: PickKind;
  target: PickTarget;
  /**
   * World-space hit; `null` on `drag` and `dragEnd`, which follow the pointer off their target. To drag
   * along a surface, use `intersectPlane` with the event's `screen`.
   */
  point: Vec3 | null;
  normal: Vec3 | null;
  /** As `MouseEvent.button`; `null` on `over`, `out` and `move`. */
  button: 0 | 1 | 2 | null;
  modifiers: { shift: boolean; ctrl: boolean; alt: boolean; meta: boolean };
  /** CSS pixels from the canvas's top left. */
  screen: [number, number];
}

/** `#rrggbb`, or `#rrggbbaa` for a translucent colour. */
export type HexColor = string;

/** A world-space axis-aligned box. */
export interface WorldAabb {
  min: Vec3;
  max: Vec3;
}

/** In the XZ plane. */
export type ZoneShape =
  /** Filled as a fan from the first point, so it must be convex. */
  | { type: 'polygon'; points: [number, number][] }
  | { type: 'circle'; center: [number, number]; radius: number };

/** An object instance as `WorldDiff.objects.instances` holds it. */
export interface OverlayObject {
  id: string;
  kindId: string;
  /** Voxels. */
  position: Vec3;
  yawDeg?: number;
}

/**
 * Drawn over a view's world. `tag` names it across `setOverlays` calls, and picks on it report
 * `{ type: 'overlay', tag }`; lines and labels are never hit. With `pickable: false`, picks pass
 * through it, as tool ghosts need.
 */
export type Overlay = { tag: string; pickable?: boolean } & (
  /** Drawn through other geometry. Edges shared by flush boxes are drawn once. */
  | { kind: 'outline'; boxes?: WorldAabb[]; segments?: [Vec3, Vec3][]; color: HexColor }
  /** The objects drawn translucent with the view's kinds (sent with `setWorld`). */
  | { kind: 'ghost'; objects: OverlayObject[]; opacity: number; pulse?: boolean; tint?: HexColor }
  /** `EndlessGrid`: lines every `cellSize`, a major one every 10, faded out by `fade` metres from the origin. */
  | { kind: 'grid'; y: number; cellSize: number; fade: number }
  /** `DirectionGizmo`: a 1.5 m arrow from `at`, drawn through other geometry. */
  | { kind: 'directionGizmo'; at: Vec3; direction: Vec3; color: HexColor }
  /** A spawn point's cone, standing on `pos`. */
  | { kind: 'marker'; shape: 'spawnCone'; pos: Vec3; yaw: number; color: HexColor }
  /** A flat area at height `y`, drawn through other geometry. */
  | { kind: 'zone'; shape: ZoneShape; y: number; color: HexColor }
  /** White text centred on `at`. */
  | { kind: 'label'; at: Vec3; text: string }
);

/** `normal · p + constant = 0`, as three.js's `Plane`. */
export interface Plane {
  normal: Vec3;
  constant: number;
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
  /**
   * Sends `callback` the view's pick events, from pointer events on its canvas, replacing any callback
   * it had; `null` stops them. A view without a callback does no picking. A throwing callback is logged
   * with `console.error` and the frame's other events are still delivered. Throws for a destroyed view.
   * A left press on any target but the ground doesn't take the controls' pointer lock.
   */
  onPick(id: ViewId, callback: ((event: PickEvent) => void) | null): void;
  /**
   * The view camera's ray through `(x, y)`, in CSS pixels from its canvas's top left, from the pose last
   * drawn (the controls' when they drive it) or a `setCamera` since.
   */
  rayAtScreen(id: ViewId, x: number, y: number): { origin: Vec3; dir: Vec3 };
  /** Where that ray meets `plane` ahead of the camera, or `null`. Throws for a zero normal. */
  intersectPlane(id: ViewId, x: number, y: number, plane: Plane): Vec3 | null;
  /** Replaces the view's overlays; only those whose `tag` is new or whose content changed are redrawn. */
  setOverlays(id: ViewId, overlays: Overlay[]): void;

  // Controls: once any of these is called on a view, its controls drive its camera, overriding
  // `setCamera`, in every frame its mode has something to frame. Top-down and free-fly always do;
  // first-person, third-person and fixed need a follow target (fixed, or its `lookAt`), and until
  // then `setCamera` still applies. Keys are heard page-wide and reach every view with input
  // enabled; pointer lock, mouse-look and the wheel reach only a view drawing into a Bevy window
  // (with WebGL2, the primary canvas's).
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
