#!/usr/bin/env node
// Fails when three.js, react-three-fiber or Leva creep back in: an import,
// `require` or `/// <reference types>` of `three`, `@react-three/*` or `leva`,
// or one of them as a dependency. Everything renders through Bevy
// (`@officexr/render-wasm`).
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE_EXTS = new Set(['.ts', '.tsx', '.mts', '.cts', '.js', '.jsx', '.mjs', '.cjs']);
const SKIP_DIRS = new Set(['node_modules', 'dist', '.vite', 'test-results', 'playwright-report']);
// wasm-pack output, generated from the Rust crates.
const SKIP_PATHS = new Set(['packages/render-wasm/pkg', 'packages/sim-wasm/pkg']);

const BANNED = String.raw`three|three/[^'"\x60]*|@react-three/[^'"\x60]*|leva`;
const SPECIFIER = String.raw`['"\x60](?:${BANNED})['"\x60]`;
const CODE_RULES = [
  new RegExp(String.raw`\b(?:from|import)\s*\(?\s*${SPECIFIER}`, 'g'),
  new RegExp(String.raw`\brequire\s*\(\s*${SPECIFIER}`, 'g'),
];
const REFERENCE_RULE = new RegExp(String.raw`///\s*<reference\s+types\s*=\s*${SPECIFIER}`, 'g');
const BANNED_DEPENDENCY = /^(three|@types\/three|@react-three\/.+|leva)$/;
const DEPENDENCY_FIELDS = ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'];

const BANNED_STRING = new RegExp(String.raw`^${SPECIFIER}$`);
const blank = (text) => text.replace(/[^\n]/g, ' ');

/**
 * `source` with its comments, and every string but a banned specifier, blanked out (newlines
 * kept, so offsets keep their line numbers), so `from 'three'` inside either can't match.
 */
function codeOnly(source) {
  return source.replace(
    /("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|`(?:\\.|[^`\\])*`)|\/\*[\s\S]*?\*\/|\/\/[^\n]*/g,
    (match, string) =>
      string === undefined
        ? blank(match)
        : BANNED_STRING.test(string)
          ? string
          : string[0] + blank(string.slice(1, -1)) + string[0],
  );
}

function lineOf(source, index) {
  return source.slice(0, index).split('\n').length;
}

function* walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name) && !SKIP_PATHS.has(relative(ROOT, path))) {
        yield* walk(path);
      }
    } else if (entry.isFile()) {
      yield path;
    }
  }
}

function checkSource(file, source) {
  const hits = [];
  const code = codeOnly(source);
  for (const rule of CODE_RULES) {
    for (const match of code.matchAll(rule)) {
      hits.push(`${file}:${lineOf(code, match.index)}: ${match[0].replace(/\s+/g, ' ')}`);
    }
  }
  for (const match of source.matchAll(REFERENCE_RULE)) {
    hits.push(`${file}:${lineOf(source, match.index)}: ${match[0]}`);
  }
  return hits;
}

function checkPackageJson(file, source) {
  const pkg = JSON.parse(source);
  return DEPENDENCY_FIELDS.flatMap((field) =>
    Object.keys(pkg[field] ?? {})
      .filter((name) => BANNED_DEPENDENCY.test(name))
      .map((name) => `${file}: ${field}.${name}`),
  );
}

const rootFiles = readdirSync(ROOT, { withFileTypes: true })
  .filter((entry) => entry.isFile())
  .map((entry) => join(ROOT, entry.name));
const files = [...rootFiles, ...['packages', 'tests', 'scripts'].flatMap((dir) => [...walk(join(ROOT, dir))])];

const hits = files.flatMap((path) => {
  const file = relative(ROOT, path);
  if (path.endsWith('package.json')) {
    return checkPackageJson(file, readFileSync(path, 'utf8'));
  }
  return SOURCE_EXTS.has(extname(path)) ? checkSource(file, readFileSync(path, 'utf8')) : [];
});

if (hits.length > 0) {
  console.error(hits.join('\n'));
  console.error('\nthree.js, react-three-fiber and Leva are gone: render through Bevy (see CLAUDE.md).');
  process.exit(1);
}
console.log(`lint:no-three clean (${files.length} files)`);
