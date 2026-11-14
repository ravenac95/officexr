// The loader's logic, free of imports from the generated pkg/ so it can be tested without a wasm build.

/** WebGPU when the browser hands out an adapter, else WebGL2. */
export async function pickBackend(gpu) {
  try {
    return (await gpu?.requestAdapter()) ? 'webgpu' : 'webgl2';
  } catch {
    return 'webgl2';
  }
}

/**
 * Instantiates the renderer for the best backend: from `<baseUrl>/<hash>/<backend>/` when `baseUrl`
 * is set, else from the bundled `bundled[backend]()` import.
 */
export async function loadRendererFrom({ gpu, baseUrl, hash, importModule, bundled }) {
  const backend = await pickBackend(gpu);
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
