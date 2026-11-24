import { useEffect, useState } from 'react';
import type { OfficeState, Store } from '@officexr/sdk';
import { frozenAvatars, sameFrozenAvatars, type FrozenAvatar } from '../app/frozen-avatars.ts';
import { useApplication, useCatalogCharacters } from './application-context.tsx';

/** The away claimants to leave sitting at their desks, re-read only when one of them changes. */
export function useFrozenAvatars(store: Store): FrozenAvatar[] {
  const { catalog, geometry } = useApplication();
  const characters = useCatalogCharacters();
  const [avatars, setAvatars] = useState<FrozenAvatar[]>(() => frozenAvatars(store.getState(), catalog, geometry, characters));
  useEffect(() => {
    const select = (s: OfficeState) => frozenAvatars(s, catalog, geometry, characters);
    setAvatars((prev) => {
      const next = select(store.getState());
      return sameFrozenAvatars(prev, next) ? prev : next;
    });
    return store.subscribe(select, setAvatars, sameFrozenAvatars);
  }, [store, catalog, geometry, characters]);
  return avatars;
}
