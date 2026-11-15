import type { Gpu, RenderBackend } from './index.js';

export { pickBackend } from './index.js';

/**
 * The backend the user forced: `?renderer=webgl2|webgpu` in `search` wins and is remembered in
 * `storage`; `?renderer=auto` forgets it. Else the remembered one, if any.
 */
export function rendererOverride(
  search: string | undefined,
  storage?: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>,
): RenderBackend | undefined;

/** True on iPhone/iPad (every browser there runs WebKit), including iPadOS's Mac-like UA. */
export function isIosWebKit(nav?: Pick<Navigator, 'userAgent' | 'maxTouchPoints'> | null): boolean;

/** `override`, else WebGL2 on iOS WebKit (its WebGPU canvas flickers before iOS 26.4), else undefined. */
export function preferredBackend(
  override: RenderBackend | undefined,
  nav?: Pick<Navigator, 'userAgent' | 'maxTouchPoints'> | null,
): RenderBackend | undefined;

export function loadRendererFrom<M extends { default(arg?: unknown): Promise<unknown> }>(deps: {
  gpu?: Gpu | null;
  /** Skips detection and loads this backend. */
  override?: RenderBackend;
  baseUrl?: string;
  hash: () => Promise<string>;
  importModule: (url: string) => Promise<M>;
  bundled?: Record<RenderBackend, () => Promise<M>>;
}): Promise<M>;
