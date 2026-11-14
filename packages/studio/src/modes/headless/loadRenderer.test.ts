import { describe, expect, it } from 'vitest';
import { loadRendererFrom, pickBackend } from '@officexr/render-wasm/load';

const adapter = { requestAdapter: async () => ({}) };
const bundled = {
  webgpu: async () => ({ default: async (_arg?: unknown) => {}, backend: 'webgpu' }),
  webgl2: async () => ({ default: async (_arg?: unknown) => {}, backend: 'webgl2' }),
};
const noAdapter = { requestAdapter: async () => null };

describe('pickBackend', () => {
  it('uses WebGPU only when an adapter comes back', async () => {
    expect(await pickBackend(adapter)).toBe('webgpu');
    expect(await pickBackend(noAdapter)).toBe('webgl2');
    expect(await pickBackend(undefined)).toBe('webgl2');
    expect(await pickBackend({ requestAdapter: async () => Promise.reject(new Error('lost')) })).toBe('webgl2');
  });
});

describe('loadRendererFrom', () => {
  it('imports the hashed glue module and points init at the matching wasm', async () => {
    const imported: string[] = [];
    const inits: unknown[] = [];
    await loadRendererFrom({
      gpu: adapter,
      baseUrl: 'https://cdn.example/wasm/',
      hash: async () => 'abc123',
      bundled: bundled,
      importModule: async (url) => {
        imported.push(url);
        return { default: async (arg?: unknown) => void inits.push(arg), backend: 'x' };
      },
    });
    expect(imported).toEqual(['https://cdn.example/wasm/abc123/webgpu/officexr_render_web.js']);
    expect(inits).toEqual([{ module_or_path: 'https://cdn.example/wasm/abc123/webgpu/officexr_render_web_bg.wasm' }]);
  });

  it('falls back to the WebGL2 directory without a GPU adapter', async () => {
    const imported: string[] = [];
    await loadRendererFrom({
      gpu: noAdapter,
      baseUrl: 'https://cdn.example/wasm',
      hash: async () => 'abc123',
      bundled: bundled,
      importModule: async (url) => {
        imported.push(url);
        return { default: async (_arg?: unknown) => {}, backend: 'x' };
      },
    });
    expect(imported).toEqual(['https://cdn.example/wasm/abc123/webgl2/officexr_render_web.js']);
  });

  it('uses the bundled build when no base URL is set', async () => {
    const renderer = await loadRendererFrom({
      gpu: adapter,
      hash: async () => 'abc123',
      importModule: async () => {
        throw new Error('must not fetch');
      },
      bundled: bundled,
    });
    expect(renderer).toMatchObject({ backend: 'webgpu' });
  });
});
