// @officexr/world — shared physics + bots + characters + scenes for the
// web app and the studio. The Bevy views live under `@officexr/world/react`.
//
// Sub-paths give callers a finer-grained import surface:
//   import { BevyPreview } from '@officexr/world/react';
//   import { BotPool } from '@officexr/world/bot';
//
// The root re-exports the most commonly used symbols.

export { VOXEL_SIZE } from './renderer/config.ts';
export * from './bot/index.ts';
export * from './physics/index.ts';
export * from './characters/index.ts';
export * from './scenes/index.ts';
