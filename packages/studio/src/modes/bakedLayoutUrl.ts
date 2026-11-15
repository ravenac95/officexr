import { bakedLayoutUrl } from '@officexr/world/app';
import { bakedLayoutGlbUrl } from '@officexr/world/react';
import { paths, studioApiUrl } from '../apiOrigin.ts';

/** A baked layout's GLB on the studio API, which wants `studioFetch`'s credentials. */
export const studioBakedLayoutUrl = (layoutName: string) =>
  bakedLayoutGlbUrl(bakedLayoutUrl(layoutName, studioApiUrl(paths.bakedLayouts)));
