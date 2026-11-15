import { useEffect, useState } from 'react';
import { bakedLayoutUrl } from '@officexr/world/app';
import { bakedLayoutGlbUrl } from '@officexr/world/react';
import { paths, studioApiUrl, studioFetch } from '../apiOrigin.ts';

/** A baked layout's GLB on the studio API, which wants `studioFetch`'s credentials. */
export const studioBakedLayoutUrl = (layoutName: string) =>
  bakedLayoutGlbUrl(bakedLayoutUrl(layoutName, studioApiUrl(paths.bakedLayouts)));

/**
 * Each baked layout GLB at `urls` as a `blob:` URL Bevy can load, by URL, once it has arrived: the
 * studio API wants its session cookie or preview token, which Bevy's own fetch doesn't send. A GLB
 * that fails to load is warned about and left out.
 */
export function useBakedLayoutBlobs(urls: readonly string[]): ReadonlyMap<string, string> {
  const [blobs, setBlobs] = useState<ReadonlyMap<string, string>>(new Map());
  // ponytail: any change to the set refetches every GLB (the browser cache serves the repeats).
  const key = [...new Set(urls)].sort().join('\n');
  useEffect(() => {
    if (!key) return;
    const made: string[] = [];
    let cancelled = false;
    for (const url of key.split('\n')) {
      studioFetch(url)
        .then((r) => (r.ok ? r.blob() : Promise.reject(new Error(`baked layout ${url}: HTTP ${r.status}`))))
        .then((b) => {
          if (cancelled) return;
          const blob = URL.createObjectURL(b);
          made.push(blob);
          setBlobs((prev) => new Map(prev).set(url, blob));
        })
        .catch((e: unknown) => console.warn(e));
    }
    return () => {
      cancelled = true;
      setBlobs(new Map());
      made.forEach((blob) => URL.revokeObjectURL(blob));
    };
  }, [key]);
  return blobs;
}
