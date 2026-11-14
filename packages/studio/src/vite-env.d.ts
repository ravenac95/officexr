/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Origin that serves the studio's `models/**` and baked layouts, when it isn't the page's own origin. */
  readonly VITE_ASSET_BASE_URL?: string;
  /** Where the render wasm builds are served from (`<base>/<content-hash>/<backend>/`); unset means the bundled build. */
  readonly VITE_RENDER_WASM_BASE_URL?: string;
  /** The studio backend's public origin, when it isn't the page's own origin. */
  readonly VITE_STUDIO_API_ORIGIN?: string;
  /** The web backend's origin, which serves Google sign-in. */
  readonly VITE_API_ORIGIN?: string;
}
