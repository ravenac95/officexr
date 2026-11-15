#!/usr/bin/env node
// Fails when three.js, react-three-fiber or Leva creep back in: an import of
// `three`, `@react-three/*` or `leva`, a Leva API name, or one of them as a
// dependency. Everything renders through Bevy (`@officexr/render-wasm`).
import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BANNED = String.raw`(three|three/[^'"]*|@react-three/[^'"]*|leva)`;

const RULES = [
  {
    name: 'three / @react-three / leva import',
    pattern: String.raw`(from|import|require\()\s*\(?\s*['"]${BANNED}['"]`,
    include: ['*.ts', '*.tsx', '*.js', '*.mjs', '*.cjs'],
  },
  { name: 'Leva API', pattern: String.raw`\b(useControls|levaPersistence|levaStore)\b`, include: ['*.ts', '*.tsx'] },
  {
    name: 'three / @react-three / leva dependency',
    pattern: String.raw`"(three|@types/three|@react-three/[^"]+|leva)"\s*:`,
    include: ['package.json'],
  },
];

let failed = false;
for (const rule of RULES) {
  const args = ['-rEn', rule.pattern, 'packages', 'tests', 'package.json', '--exclude-dir=node_modules', '--exclude-dir=dist', '--exclude-dir=pkg'];
  for (const glob of rule.include) {
    args.push(`--include=${glob}`);
  }
  try {
    const out = execFileSync('grep', args, { cwd: ROOT, encoding: 'utf8' });
    console.error(`=== ${rule.name} ===\n${out}`);
    failed = true;
  } catch (err) {
    // grep exits 1 when nothing matches.
    if (err.status !== 1) {
      console.error(`[lint:no-three] grep failed for "${rule.name}": ${err.message}`);
      process.exit(2);
    }
  }
}

if (failed) {
  console.error('three.js, react-three-fiber and Leva are gone: render through Bevy (see CLAUDE.md).');
  process.exit(1);
}
console.log('lint:no-three clean');
