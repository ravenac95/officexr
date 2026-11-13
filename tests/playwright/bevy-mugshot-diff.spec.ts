/**
 * Evidence, not a gate: renders the Barbarian mugshot manifest with
 * `crates/officexr-render`'s `render --mugshot` in both cube modes from the
 * four angles, and reports `pixelDiffCount` against each ideal PNG with
 * `mugshot-baseline-compare.spec.ts`'s threshold. Writes each Bevy PNG, a
 * side-by-side with its ideal, and `result.json`.
 *
 * OFFICEXR_BEVY_DIFF=<out dir> pnpm exec playwright test tests/playwright/bevy-mugshot-diff.spec.ts
 */

import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { test } from '@playwright/test';
import { PNG } from 'pngjs';
import { pixelDiffCount } from './helpers.ts';

const OUT_DIR = process.env.OFFICEXR_BEVY_DIFF;
const ROOT = path.resolve(__dirname, '../..');
const BASELINE = path.join(__dirname, 'mugshot-baselines', 'Barbarian');
const PIXEL_THRESHOLD = 6;
const NEAR = 0.1;
const FAR = 2000;
const ANGLES = [
  { name: 'north', deg: 0 },
  { name: 'east', deg: 90 },
  { name: 'south', deg: 180 },
  { name: 'west', deg: 270 },
] as const;
const VOXELS = [
  [-4, -2, -4],
  [0, -2, -4],
  [-4, -2, 0],
  [0, -2, 0],
] as const;
// MugshotApp.tsx's cube tables: blue/stone on the diagonals in both modes.
const CUBE_KINDS = {
  gltf: ['colored_block_blue', 'stone', 'stone', 'colored_block_blue'],
  primitive: ['__primitive_blue', '__primitive_stone', '__primitive_stone', '__primitive_blue'],
} as const;

interface Manifest {
  character: string;
  fixedCamera: { azimuthDeg: number; pitchDeg: number; height: number; fov: number; distanceM: number };
  lighting: object;
  background: object;
}

/** CameraRig's fixed leash camera around `lookAt: [0, 0, 0]`. */
function mugshotCamera(fixed: Manifest['fixedCamera'], azimuthDeg: number) {
  const az = (azimuthDeg * Math.PI) / 180;
  const pitch = (fixed.pitchDeg * Math.PI) / 180;
  const position = [Math.sin(az) * fixed.distanceM, fixed.height, -Math.cos(az) * fixed.distanceM];
  const target = [
    position[0] - Math.sin(az) * Math.cos(pitch),
    position[1] + Math.sin(pitch),
    position[2] + Math.cos(az) * Math.cos(pitch),
  ];
  return { position, target };
}

test.skip(!OUT_DIR, 'set OFFICEXR_BEVY_DIFF=<out dir> to run');

test('Bevy render of the Barbarian mugshot vs its ideal PNGs', () => {
  test.setTimeout(30 * 60_000);
  const out = (name: string) => path.resolve(OUT_DIR!, name);
  fs.mkdirSync(OUT_DIR!, { recursive: true });
  const manifest = JSON.parse(fs.readFileSync(path.join(BASELINE, 'manifest.json'), 'utf-8')) as Manifest;
  const catalog = JSON.parse(
    fs.readFileSync(path.join(ROOT, 'packages/world/world-object-kinds.json'), 'utf-8'),
  ) as { kinds: { id: string }[] };

  const results = [];
  for (const mode of ['gltf', 'primitive'] as const) {
    const kindIds = CUBE_KINDS[mode];
    const scene = out(`${mode}.scene.json`);
    fs.writeFileSync(
      scene,
      JSON.stringify({
        kinds: catalog.kinds.filter((k) => (kindIds as readonly string[]).includes(k.id)),
        objects: {
          cubeSize: 0.5,
          instances: VOXELS.map((position, i) => ({ id: `m-${i}`, kindId: kindIds[i], position })),
        },
        lighting: manifest.lighting,
        background: manifest.background,
      }),
    );
    for (const angle of ANGLES) {
      const file = mode === 'gltf' ? `${angle.name}.png` : `primitive-${angle.name}.png`;
      const { position, target } = mugshotCamera(manifest.fixedCamera, angle.deg);
      const ideal = PNG.sync.read(fs.readFileSync(path.join(BASELINE, 'ideal', file)));
      execFileSync(
        'cargo',
        [
          'run', '-q', '-p', 'officexr-render', '--features', 'mugshot', '--bin', 'render', '--',
          '--mugshot', manifest.character,
          // The ideals are the canvas inside the manifest's bordered viewport.
          scene, out(file), `${ideal.width}x${ideal.height}`,
          position.join(','), target.join(','),
          String(manifest.fixedCamera.fov), String(NEAR), String(FAR),
        ],
        { cwd: ROOT, stdio: ['ignore', 'inherit', 'inherit'] },
      );
      const bevy = PNG.sync.read(fs.readFileSync(out(file)));
      const side = new PNG({ width: ideal.width * 2, height: ideal.height });
      PNG.bitblt(ideal, side, 0, 0, ideal.width, ideal.height, 0, 0);
      PNG.bitblt(bevy, side, 0, 0, bevy.width, bevy.height, ideal.width, 0);
      fs.writeFileSync(out(`side-by-side-${file}`), PNG.sync.write(side));
      const diff = pixelDiffCount(ideal.data, bevy.data, PIXEL_THRESHOLD);
      results.push({ baseline: file, pixelDiffCount: diff, totalPixels: ideal.width * ideal.height });
    }
  }
  fs.writeFileSync(out('result.json'), `${JSON.stringify({ pixelThreshold: PIXEL_THRESHOLD, results }, null, 2)}\n`);
  console.log(JSON.stringify(results));
});
