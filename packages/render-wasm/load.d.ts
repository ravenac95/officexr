import type { Gpu, RenderBackend } from './index.js';

export { pickBackend } from './index.js';

export function loadRendererFrom<M extends { default(arg?: unknown): Promise<unknown> }>(deps: {
  gpu?: Gpu | null;
  baseUrl?: string;
  hash: () => Promise<string>;
  importModule: (url: string) => Promise<M>;
  bundled?: Record<RenderBackend, () => Promise<M>>;
}): Promise<M>;
