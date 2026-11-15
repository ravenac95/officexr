// The loader's logic, free of imports from the generated pkg/ so it can be tested without a wasm build.

const BACKENDS = ['webgpu', 'webgl2'];
const STORAGE_KEY = 'officexr:renderer';

/**
 * The backend the user forced: `?renderer=webgl2|webgpu` wins and is remembered in `storage`, so it
 * survives navigating to a URL without it; `?renderer=auto` forgets it. Else the remembered one, if any.
 */
export function rendererOverride(search, storage) {
  const asked = new URLSearchParams(search ?? '').get('renderer');
  try {
    if (asked === 'auto') {
      storage?.removeItem(STORAGE_KEY);
      return undefined;
    }
    if (BACKENDS.includes(asked)) {
      storage?.setItem(STORAGE_KEY, asked);
      return asked;
    }
    const stored = storage?.getItem(STORAGE_KEY);
    return BACKENDS.includes(stored) ? stored : undefined;
  } catch {
    // Storage blocked (private mode, sandboxed frame): only the URL counts.
    return BACKENDS.includes(asked) ? asked : undefined;
  }
}

/**
 * `override` when given, else WebGPU when the browser hands out an adapter that opens a device,
 * else WebGL2. The probe device is destroyed at once; Bevy makes its own.
 */
export async function pickBackend(gpu, override) {
  if (override) {
    return override;
  }
  try {
    const adapter = await gpu?.requestAdapter();
    if (!adapter) {
      return 'webgl2';
    }
    const device = await adapter.requestDevice();
    device.destroy?.();
    return 'webgpu';
  } catch {
    return 'webgl2';
  }
}

/**
 * Instantiates the renderer for the best backend (or `override`): from `<baseUrl>/<hash>/<backend>/`
 * when `baseUrl` is set, else from the bundled `bundled[backend]()` import.
 */
export async function loadRendererFrom({ gpu, override, baseUrl, hash, importModule, bundled }) {
  const backend = await pickBackend(gpu, override);
  // ponytail: WebGPU failing only once Bevy runs (its own device's limits, a lost device) does not fall back to WebGL2.
  globalThis.__officexrRenderBackend = backend;
  if (!baseUrl) {
    const renderer = await bundled[backend]();
    await renderer.default();
    return renderer;
  }
  const dir = `${baseUrl.replace(/\/+$/, '')}/${await hash()}/${backend}`;
  const renderer = await importModule(`${dir}/officexr_render_web.js`);
  await renderer.default({ module_or_path: `${dir}/officexr_render_web_bg.wasm` });
  return renderer;
}
