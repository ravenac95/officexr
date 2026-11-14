/**
 * Drops the bundled renderer wasm from a build that loads it from `VITE_RENDER_WASM_BASE_URL`: the glue's
 * `new URL(...wasm)` emits it even though `loadRenderer` never reaches it, and it's too big for Pages.
 */
export function dropBundledRenderWasm() {
  let remote = false;
  return {
    name: 'officexr:drop-render-wasm',
    apply: 'build',
    configResolved(config) {
      remote = Boolean(config.env.VITE_RENDER_WASM_BASE_URL);
    },
    generateBundle(_, bundle) {
      if (!remote) {
        return;
      }
      for (const name of Object.keys(bundle)) {
        if (/officexr_render_web_bg.*\.wasm$/.test(name)) {
          delete bundle[name];
        }
      }
    },
  };
}
