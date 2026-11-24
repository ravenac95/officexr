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
  /** A flat background instead of the sky (`#rrggbbaa` for a see-through one); `null` brings the sky back. */
  clearColor?: string | null;
}

export type Vec3 = [number, number, number];

/** What a pick hit; events bubble from a mesh up to the nearest target. */
export type PickTarget =
  | { type: 'object'; id: string }
  | { type: 'layout'; name: string; roomInstanceId: string }
  | { type: 'player'; id: string }
  /** The bell shown over a hovered player; only `click` is reported on it. */
  | { type: 'pingBell'; id: string }
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

/** One frame's `TrackerOutput`, as `setProximity` takes it. */
export interface ProximityInput {
  /** Each player's disc state; a player left out shows no disc. */
  players: Record<string, 'entering' | 'entered' | 'exiting'>;
  areas: { id: string; center: [number, number]; radius: number; members: string[] }[];
  /** The feet of players `setPlayers` left out, as the first-person local player. */
  undrawn?: Record<string, [number, number, number]>;
  /** The players bubbles rise around: both ends of each of the local player's `entering` pairs. */
  bubbles?: string[];
  /** The world constants' `proximity` tunables; each defaults to `DEFAULT_WORLD_CONSTANTS`'. */
  style?: {
    discRadius?: number;
    pulseSpeed?: number;
    intensity?: number;
    enteringColor?: HexColor;
    enteredColor?: HexColor;
    exitingColor?: HexColor;
    meetingBorderInset?: number;
    meetingBorderOutset?: number;
    sparkleSpeed?: number;
    sparkleFloatHeight?: number;
    /** With the camera mode's multiplier already applied: the room view uses 5 in the fixed camera. */
    sparkleSize?: number;
  };
}

/** An object instance as `WorldDiff.objects.instances` holds it. */
export interface OverlayObject {
  id: string;
  kindId: string;
  /** Voxels. */
  position: Vec3;
  yawDeg?: number;
}

/**
 * Drawn over a view's world. `tag` names it across `setOverlays` calls. A pickable overlay's picks
 * report `{ type: 'overlay', tag }`; lines and labels are never hit. An unpickable one lets picks
 * through to what lies behind. `pickable` defaults to `true` for markers only: the map editor
 * selects spawn points by clicking them. Pass `true` for clickable desk areas; leave the grid
 * unpickable, or it takes every floor click from the ground.
 */
export type Overlay = { tag: string; pickable?: boolean } & (
  /** Drawn through other geometry. Edges shared by flush boxes are drawn once. */
  | { kind: 'outline'; boxes?: WorldAabb[]; segments?: [Vec3, Vec3][]; color: HexColor }
  /**
   * The objects drawn translucent with the view's kinds: each `kindId` must be in a `setWorld` diff's
   * `kinds`. A kind a ghost has used stays loaded while it is there, so the ghost shows at once when
   * it comes back.
   */
  | { kind: 'ghost'; objects: OverlayObject[]; opacity: number; pulse?: boolean; tint?: HexColor }
  /** An endless grid: lines every `cellSize`, a major one every 10, faded out by `fade` metres from the origin. */
  | { kind: 'grid'; y: number; cellSize: number; fade: number }
  /** `DirectionGizmo`: a 1.5 m arrow from `at`, drawn through other geometry. */
  | { kind: 'directionGizmo'; at: Vec3; direction: Vec3; color: HexColor }
  /** A spawn point's cone, standing on `pos`; `selected` makes it glow in `color` under a yellow tip. */
  | { kind: 'marker'; shape: 'spawnCone'; pos: Vec3; yaw: number; color: HexColor; selected?: boolean }
  /** A flat area at height `y`, drawn through other geometry. */
  | { kind: 'zone'; shape: ZoneShape; y: number; color: HexColor }
  /** White text centred on `at`. */
  | { kind: 'label'; at: Vec3; text: string }
  /**
   * A translucent column rising `height` (default 6 m) from the floor point `at`, a ring at its
   * foot and, unless empty, `label` on a `color` pill over its top. Moving it or changing the
   * label's text updates it in place.
   */
  | { kind: 'beacon'; at: Vec3; color: HexColor; label?: string; height?: number }
);

/** What the play camera must keep in sight this frame (`setOcclusion`). */
export interface OcclusionInput {
  /** World points the camera must see, e.g. the local player's head and chest. */
  targets: Vec3[];
  /** Object ids that never occlude: the seat, ridden rack or pool the player is in. */
  ignore?: string[];
  /** A point the player occupies (seat, ride, pool): an object holding it never occludes. */
  anchor?: Vec3 | null;
  /** The local player's feet: set, the cover overhead is cut away around them (roof cutaway). */
  cutaway?: Vec3 | null;
  /** Other players in a seat, ride or pool: `occlusionStats` names those only that hides. */
  occupants?: { id: string; targets: Vec3[]; ignore?: string[]; anchor?: Vec3 | null }[];
}

export interface OcclusionStats {
  /** Objects and layout meshes drawn faded or cut away. */
  fading: number;
  /** Those cut away over the player. */
  cutaway: number;
  /** The `occupants` nothing but their own seat, ride or pool hides: their silhouette shouldn't show. */
  silhouetteSuppressed: string[];
}

/** A pool's water, as `motion/water.ts`'s `WaterVolume`. */
export interface WaterVolumeInput {
  id: string;
  minX: number;
  maxX: number;
  minZ: number;
  maxZ: number;
  baseY?: number;
  surfaceY: number;
}

/** A desk screen to light (`setScreens`). */
export interface ScreenInput {
  /** The object instance that is the screen. */
  id: string;
  /** 0 off to 1 fully on. */
  glow: number;
  /** World XZ from the screen toward its chair: the model's broad face pointing this way lights. */
  toward: [number, number];
}

/** The plane `normal · p + constant = 0`. */
export interface Plane {
  normal: Vec3;
  constant: number;
}

export type CameraMode = 'first-person' | 'third-person' | 'fixed' | 'top-down' | 'free-fly' | 'orbit';

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
    /** Keeps the followed player's chest at the screen centre, ignoring the conversation. Ignored with `lookAt`. */
    centred?: boolean;
  };
  /** The orbit mode. The defaults are the scene editor's. */
  orbit?: {
    fov?: number;
    minDistance?: number;
    maxDistance?: number;
    /** Positive looks down on the target from above. */
    minElevationDeg?: number;
    maxElevationDeg?: number;
    /**
     * `right`, the scene editor's: right-drags rotate, shift+right- and middle-drags pan, and left is
     * the editor's. `left`: left-drags rotate, right-drags pan, ctrl, cmd or shift
     * swaps the two, and middle-drags zoom.
     */
    rotateButton?: 'left' | 'right';
    pan?: boolean;
    /** Time constant (s) the camera eases toward where input puts it; 0 snaps. Zoom never eases. */
    dampingSecs?: number;
    /** WASD moves the target across the ground and Q/E up and down; `takeIntent` then stays still. */
    moveKeys?: boolean;
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
  /** The stick's deflection when it, not the keys, gave `dir`; else 0. */
  deflection: number;
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
  /**
   * `players`: `{ players, charRadius, playerSpeed, runSpeed, bumpEasingMs?, paused? }`. A player's `animation`, when set,
   * plays that state's clip at that time scale instead of the one its velocity implies. A player's optional
   * `seat` (`{ pos, yaw }`) sits it there; a change of its `bump.count` plays the hit reaction and a kick along
   * `bump.normal`, eased over `bumpEasingMs`. A player's optional `animSpeeds` (`{ idle, walk, run }`, each 1 when
   * omitted) multiplies its clip rate: a walk or run plays at its ground speed over the clip's authored ground
   * speed, so at 1 the planted foot doesn't slide. `paused` draws every player in its bind pose without name tags,
   * as a `setWorld` mugshot does.
   */
  setPlayers(id: ViewId, players: unknown): void;
  setCamera(id: ViewId, pose: CameraPose): void;
  /** `ready` once every model has loaded and settled; `pending` counts the models still loading. */
  isReady(id: ViewId): { ready: boolean; pending: number };
  frameStats(id: ViewId): { drawCalls: number; triangles: number };
  /** The view's next frame as a PNG, keeping a see-through `clearColor`'s alpha. Throws for a destroyed view. */
  captureView(id: ViewId): Promise<Blob>;
  /**
   * Sends `callback` the view's pick events, from pointer events on its canvas, replacing any callback
   * it had; `null` stops them. A view without a callback does no picking. A throwing callback is logged
   * with `console.error` and the frame's other events are still delivered. Throws for a destroyed view.
   * A left press on any target but the ground doesn't start the controls' mouse-look drag,
   * unless `bellClaimsOnly`: then only a press on a ping bell doesn't.
   */
  onPick(id: ViewId, callback: ((event: PickEvent) => void) | null, options?: { bellClaimsOnly?: boolean }): void;
  /**
   * The view camera's ray through `(x, y)`, in CSS pixels from its canvas's top left, from the pose last
   * drawn (the controls' when they drive it) or a `setCamera` since.
   */
  rayAtScreen(id: ViewId, x: number, y: number): { origin: Vec3; dir: Vec3 };
  /** Where that ray meets `plane` ahead of the camera, or `null`. Throws for a zero normal. */
  intersectPlane(id: ViewId, x: number, y: number, plane: Plane): Vec3 | null;
  /**
   * Replaces the view's overlays, diffed by `tag`. Moving a label, marker or gizmo, changing a label's
   * text, moving a ghost's objects or an outline's lines updates it in place; any other change redraws it.
   */
  setOverlays(id: ViewId, overlays: Overlay[]): void;
  /**
   * Where the view's camera sees `point`, in CSS pixels from its canvas's top left, from the pose last
   * drawn (as `rayAtScreen`); `null` when it is behind the camera.
   */
  projectToScreen(id: ViewId, point: Vec3): [number, number] | null;
  /**
   * Objects and baked-layout meshes crossing the camera's line of sight to `targets` fade, and with
   * `cutaway` the roof over the player is cut away; send it every frame. `null` restores them all.
   */
  setOcclusion(id: ViewId, input: OcclusionInput | null): void;
  /** What occlusion did on the view's last frame. */
  occlusionStats(id: ViewId): OcclusionStats;
  /** Replaces the view's pools' water surfaces. */
  setWater(id: ViewId, volumes: WaterVolumeInput[]): void;
  /** Replaces the view's lit desk screens; a screen left out is off. Send it as their glow changes. */
  setScreens(id: ViewId, screens: ScreenInput[]): void;
  /**
   * Replaces the view's proximity output, drawn as ground discs under the view's
   * players and its meeting-area glow. The glow eases toward it, so send it every frame.
   */
  setProximity(id: ViewId, proximity: ProximityInput): void;

  // Controls: once any of these is called on a view, its controls drive its camera, overriding
  // `setCamera`, in every frame its mode has something to frame. Top-down, free-fly and orbit always do;
  // first-person, third-person and fixed need a follow target (fixed, or its `lookAt`), and until
  // then `setCamera` still applies. Keys are heard page-wide and reach every view with input
  // enabled; mouse-look reaches only a view drawing into a Bevy window (with WebGL2, the primary
  // canvas's): in first- and third-person the right button held, or a left drag past 4 CSS px, turns
  // the view, with the pointer locked only while turning. Drags and the wheel reach every view from
  // its own canvas.
  /** The movement since the last call, to step the sim with. */
  takeIntent(id: ViewId): MoveIntent;
  /** The local player's body root and yaw, after stepping the sim. */
  setFollowTarget(id: ViewId, pos: [number, number, number], yaw: number): void;
  setCameraMode(id: ViewId, mode: CameraMode): void;
  /** The conversation's centre, or `null` outside one. */
  setConversationFocus(id: ViewId, point: [number, number, number] | null): void;
  /** Replaces the view's camera settings. */
  setCameraConfig(id: ViewId, config: CameraConfig): void;
  /**
   * Moves the orbit camera at once; a field left out keeps its value. Each frame clamps it to the
   * limits in force then, so it can come before or after `setCameraConfig`. Numbers must be finite.
   * The camera sits `distance` from `target` toward `azimuth` (its look yaw, radians) and
   * `elevation` (radians, positive above the target), as the studio editors' orbit refs do.
   */
  setOrbit(
    id: ViewId,
    pose: { target?: [number, number, number]; azimuth?: number; elevation?: number; distance?: number },
  ): void;
  /** Fits the orbit camera to the box, keeping its angles, for previews and "focus selection". */
  frameBounds(id: ViewId, aabb: { min: [number, number, number]; max: [number, number, number] }): void;
  /** Drive from `useWorldFocus` and `isTypingTarget`: off, keys and the mouse are ignored. */
  setInputEnabled(id: ViewId, enabled: boolean): void;
  /** Off, keys are ignored but drags and the wheel still move the camera, as while typing in a field beside a preview. */
  setKeysEnabled(id: ViewId, enabled: boolean): void;
  /** The page's touch stick: `x` strafe (right +), `y` forward (+), within the unit circle. Held keys win. */
  setStick(id: ViewId, stick: [number, number]): void;
  /** A `deviceorientation` event's angles; nulls forget the previous reading, so the next starts afresh. */
  setDeviceOrientation(id: ViewId, reading: { alpha: number | null; beta: number | null; gamma: number | null }): void;
  /**
   * `'bevy'` shows the view's own touch stick; `'dom'` hides it for the page's, fed by `setStick`.
   * Returns whether the view can show the Bevy stick: only the primary canvas's view can.
   */
  setJoystickKind(id: ViewId, kind: JoystickKind): boolean;
  /** On, a finger dragging on the view no longer turns it; the mouse still does. */
  setLookLocked(id: ViewId, locked: boolean): void;
}

export type JoystickKind = 'bevy' | 'dom';

export type Gpu = {
  requestAdapter(): Promise<{ requestDevice(): Promise<{ destroy?(): void }> } | null>;
};

export type RenderBackend = 'webgpu' | 'webgl2';

/** `override` when given, else WebGPU when an adapter comes back and opens a device, else WebGL2. */
export function pickBackend(gpu?: Gpu | null, override?: RenderBackend): Promise<RenderBackend>;

/**
 * Loads and instantiates the renderer: the `?renderer=webgl2|webgpu` (or remembered) override, else
 * WebGPU when an adapter is available, else WebGL2, from
 * `VITE_RENDER_WASM_BASE_URL` when set and from the bundled build otherwise.
 */
export function loadRenderer(): Promise<RendererModule>;
