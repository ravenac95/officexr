import { cachedCharacterPortrait } from '../characters/portraitCache.ts';

// ponytail: officexr-render has no offscreen capture yet, so a model with no
// baked or catalog portrait gets none and the HUD shows its initials.
const UNAVAILABLE = 'character portraits cannot be rendered until officexr-render can capture a frame';

/** The portrait for `url`: the cached one, else a rejection (see the note above). */
export function renderCharacterPortrait(url: string): Promise<string> {
  const known = cachedCharacterPortrait(url);
  return known ? Promise.resolve(known) : Promise.reject(new Error(UNAVAILABLE));
}

/** Renders `url` as a data URL of type `mime`, bypassing every cache. For the build script that bakes the built-in portraits. */
export function bakeCharacterPortrait(_url: string, _mime: string): Promise<string> {
  return Promise.reject(new Error(UNAVAILABLE));
}
