// Builds the renderer once per graphics backend into pkg/<backend>/, then writes pkg/hash.js.
import { execFileSync } from 'node:child_process';
import { rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const pkg = fileURLToPath(new URL('./pkg', import.meta.url));
const crate = fileURLToPath(new URL('../../crates/officexr-render-web', import.meta.url));

rmSync(pkg, { recursive: true, force: true });
for (const backend of ['webgpu', 'webgl2']) {
  execFileSync(
    'wasm-pack',
    ['build', crate, '--profile', 'wasm-release', '--target', 'web', '--no-pack', '--out-dir', `${pkg}/${backend}`, '--', '--features', backend],
    { stdio: 'inherit' },
  );
}
await import('./hash.mjs');
