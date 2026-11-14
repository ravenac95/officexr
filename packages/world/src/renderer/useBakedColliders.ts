import { useEffect, useEffectEvent, useState } from 'react';
import { getVersion } from '../app/bake-registry.ts';
import { readGlbColliders } from '../app/baked-collider-extras.ts';
import type { ColliderDescriptor } from '../physics/rules.ts';
import type { SceneBakedLayout } from './Scene.tsx';

/**
 * Each baked layout's layout-local colliders, read from its GLB, by room instance id.
 * A layout appears once its GLB has loaded, as null when the GLB embeds no colliders (a stale bake);
 * a failed load is reported and leaves it absent.
 * The URL carries the registry's bake version so a re-bake refetches.
 */
export function useBakedColliders(
  layouts: readonly SceneBakedLayout[] | undefined,
  onError?: (error: Error) => void,
): ReadonlyMap<string, readonly ColliderDescriptor[] | null> {
  const [loaded, setLoaded] = useState<ReadonlyMap<string, readonly ColliderDescriptor[] | null>>(new Map());
  const reportError = useEffectEvent((error: Error) => onError?.(error));
  const urls = (layouts ?? []).map((p) => {
    const version = p.layoutName ? getVersion(p.layoutName) : 0;
    return [p.roomInstanceId, version > 0 ? `${p.gltfPath}?v=${version}` : p.gltfPath] as const;
  });
  const key = JSON.stringify(urls);

  useEffect(() => {
    let live = true;
    for (const [roomInstanceId, url] of urls) {
      fetch(url)
        .then((res) => (res.ok ? res.arrayBuffer() : Promise.reject(new Error(`${url}: HTTP ${res.status}`))))
        // ponytail: the GLB is downloaded twice (here and by BakedLayout); Range requests for the header and JSON chunk would avoid it.
        .then((glb) => {
          const colliders = readGlbColliders(glb);
          if (!colliders) {
            console.warn(
              `[useBakedColliders] "${url}" carries no embedded colliders (stale bake); ` +
                'its compiled instances collide instead. Re-save the layout to re-bake it.',
            );
          }
          if (live) {
            setLoaded((prev) => new Map(prev).set(roomInstanceId, colliders));
          }
        })
        .catch((error: Error) => {
          if (live) {
            reportError(error);
          }
        });
    }
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` is `urls`
  }, [key]);

  return loaded;
}
