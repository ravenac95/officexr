// Builds the renderer once per graphics backend into pkg/<backend>/ and writes pkg/hash.js.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const BACKENDS = ['webgpu', 'webgl2'];
const pkg = fileURLToPath(new URL('./pkg', import.meta.url));
const crate = fileURLToPath(new URL('../../crates/officexr-render-web', import.meta.url));

rmSync(pkg, { recursive: true, force: true });
const hash = createHash('sha256');
for (const backend of BACKENDS) {
  execFileSync(
    'wasm-pack',
    ['build', crate, '--profile', 'wasm-release', '--target', 'web', '--no-pack', '--out-dir', `${pkg}/${backend}`, '--', '--features', backend],
    { stdio: 'inherit' },
  );
  for (const file of ['officexr_render_web.js', 'officexr_render_web_bg.wasm']) {
    hash.update(readFileSync(`${pkg}/${backend}/${file}`));
  }
  console.log(`${backend}: ${statSync(`${pkg}/${backend}/officexr_render_web_bg.wasm`).size} bytes`);
}
writeFileSync(`${pkg}/hash.js`, `export const RENDER_WASM_HASH = '${hash.digest('hex').slice(0, 16)}';\n`);
