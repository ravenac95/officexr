export { pickBackend } from './load.js';

// The built pkg/ is reached only through this dynamic import, so code that never boots the
// renderer (unit tests, the 2D view) loads without the wasm build.
export const loadRenderer = () => import('./renderer.js').then((m) => m.loadRenderer());
