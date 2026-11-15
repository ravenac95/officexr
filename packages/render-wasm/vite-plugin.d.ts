/** A Vite plugin, typed structurally so this package needn't depend on Vite. */
export function dropBundledRenderWasm(): {
  name: string;
  apply: 'build';
  configResolved(config: { env: Record<string, unknown> }): void;
  generateBundle(options: unknown, bundle: Record<string, unknown>): void;
};
