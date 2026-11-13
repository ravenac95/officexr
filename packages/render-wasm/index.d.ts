/**
 * Hand-written types for `crates/officexr-render-web`'s wasm-bindgen API, so
 * the studio typechecks without the Bevy wasm build. Keep in step with the
 * `#[wasm_bindgen]` exports in that crate's `src/lib.rs`.
 */

/** Loads and instantiates the wasm module; call once before anything else. */
export default function init(moduleOrPath?: unknown): Promise<unknown>;

/** Boots Bevy on the canvas matching `canvas`, loading models from `origin`. */
export function start(canvas: string, origin: string): void;

/** Replaces the scene; with `mugshot`, that character is drawn paused on it. */
export function setScene(sceneJson: string, camera: unknown, mugshot?: string | null): void;

export function setPlayers(players: unknown): void;

/** Every model of the last scene has loaded and settled. */
export function isReady(): boolean;
