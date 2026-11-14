import { loadRendererFrom } from './load.js';

export { pickBackend } from './load.js';

const baseUrl = import.meta.env?.VITE_RENDER_WASM_BASE_URL;

export const loadRenderer = () =>
  loadRendererFrom({
    gpu: globalThis.navigator?.gpu,
    baseUrl,
    hash: async () => (await import('./pkg/hash.js')).RENDER_WASM_HASH,
    importModule: (url) => import(/* @vite-ignore */ url),
    // Left out when a base URL is set, so the bundler never emits the wasm into the Pages build.
    bundled: baseUrl
      ? undefined
      : {
          webgpu: () => import('./pkg/webgpu/officexr_render_web.js'),
          webgl2: () => import('./pkg/webgl2/officexr_render_web.js'),
        },
  });
