// Writes pkg/hash.js: a content hash of every backend's built glue and wasm.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const pkg = fileURLToPath(new URL('./pkg', import.meta.url));
const hash = createHash('sha256');
for (const backend of ['webgpu', 'webgl2']) {
  for (const file of ['officexr_render_web.js', 'officexr_render_web_bg.wasm']) {
    hash.update(readFileSync(`${pkg}/${backend}/${file}`));
  }
}
writeFileSync(`${pkg}/hash.js`, `export const RENDER_WASM_HASH = '${hash.digest('hex').slice(0, 16)}';\n`);
