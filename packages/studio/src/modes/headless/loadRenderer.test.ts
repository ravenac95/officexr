import { describe, expect, it } from 'vitest';
import { loadRendererFrom, pickBackend, rendererOverride } from '@officexr/render-wasm/load';

const destroyed: unknown[] = [];
const device = { destroy: () => void destroyed.push(device) };
const adapter = { requestAdapter: async () => ({ requestDevice: async () => device }) };
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

  it('falls back to WebGL2 when the adapter cannot open a device, and frees the probe device', async () => {
    const noDevice = { requestAdapter: async () => ({ requestDevice: () => Promise.reject(new Error('OperationError')) }) };
    expect(await pickBackend(noDevice)).toBe('webgl2');
    destroyed.length = 0;
    expect(await pickBackend(adapter)).toBe('webgpu');
    expect(destroyed).toEqual([device]);
  });

  it('takes an override over what the adapter says, without asking for one', async () => {
    const unasked = {
      requestAdapter: async () => {
        throw new Error('must not ask');
      },
    };
    expect(await pickBackend(unasked, 'webgl2')).toBe('webgl2');
    expect(await pickBackend(noAdapter, 'webgpu')).toBe('webgpu');
  });
});

function memoryStorage() {
  const items = new Map<string, string>();
  return {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
    removeItem: (key: string) => void items.delete(key),
  };
}

describe('rendererOverride', () => {
  it('reads ?renderer= and remembers it for URLs without one', () => {
    const storage = memoryStorage();
    expect(rendererOverride('?renderer=webgl2', storage)).toBe('webgl2');
    expect(rendererOverride('', storage)).toBe('webgl2');
    expect(rendererOverride('?sim=rust&renderer=webgpu', storage)).toBe('webgpu');
    expect(rendererOverride('?sim=rust', storage)).toBe('webgpu');
  });

  it('forgets the remembered backend on ?renderer=auto', () => {
    const storage = memoryStorage();
    rendererOverride('?renderer=webgl2', storage);
    expect(rendererOverride('?renderer=auto', storage)).toBeUndefined();
    expect(rendererOverride('', storage)).toBeUndefined();
  });

  it('ignores unknown values and still honours the URL when storage throws', () => {
    const storage = memoryStorage();
    expect(rendererOverride('?renderer=vulkan', storage)).toBeUndefined();
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
      removeItem: () => {
        throw new Error('blocked');
      },
    };
    expect(rendererOverride('?renderer=webgl2', blocked)).toBe('webgl2');
    expect(rendererOverride('', blocked)).toBeUndefined();
    expect(rendererOverride('?renderer=webgl2', undefined)).toBe('webgl2');
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

  it('loads the overridden backend even when WebGPU is available', async () => {
    const imported: string[] = [];
    await loadRendererFrom({
      gpu: adapter,
      override: 'webgl2',
      baseUrl: 'https://cdn.example/wasm',
      hash: async () => 'abc123',
      importModule: async (url) => {
        imported.push(url);
        return { default: async (_arg?: unknown) => {}, backend: 'x' };
      },
    });
    expect(imported).toEqual(['https://cdn.example/wasm/abc123/webgl2/officexr_render_web.js']);
  });
});
