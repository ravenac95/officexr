import { useEffect, useMemo, useState } from 'react';
import { propToInstance, type PlacedProp, type Store, type WorldObjects } from '@officexr/sdk';
import type { ExcludeInstance } from '../scenes/compile-map.ts';

/**
 * Follows `store`'s `worldObjects` plus the props players placed, leaving out the instances `exclude` matches.
 * `exclude` must be referentially stable, or the result is a new object every render.
 */
export function useStoreWorldObjects(store: Store, exclude?: ExcludeInstance): WorldObjects {
  const [snapshot, setSnapshot] = useState<WorldObjects>(() => store.getState().worldObjects);
  useEffect(() => {
    // Re-read before subscribing: a `setWorldObjects` landing between the
    // lazy init (during render) and this effect (after commit) would
    // otherwise be missed, pinning the snapshot to the empty default.
    setSnapshot(store.getState().worldObjects);
    return store.subscribe((s) => s.worldObjects, setSnapshot);
  }, [store]);
  const [placed, setPlaced] = useState<PlacedProp[]>(() => store.getState().placedProps);
  useEffect(() => {
    setPlaced(store.getState().placedProps);
    return store.subscribe((s) => s.placedProps, setPlaced);
  }, [store]);
  return useMemo(() => {
    if (!exclude && placed.length === 0) {
      return snapshot;
    }
    const base = exclude ? snapshot.instances.filter((i) => !exclude(i)) : snapshot.instances;
    return { ...snapshot, instances: placed.length ? [...base, ...placed.map(propToInstance)] : base };
  }, [snapshot, exclude, placed]);
}
